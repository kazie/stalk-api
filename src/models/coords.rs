use crate::problem::FieldError;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use utoipa::ToSchema;

/// A user's latest stored position, as returned by the API and in WebSocket `update` events.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UserCoords {
    /// User name, with the casing it was first stored with.
    #[schema(example = "hannes")]
    pub name: String,
    /// Latitude in WGS84 decimal degrees.
    #[schema(example = 59.334, minimum = -90, maximum = 90)]
    pub latitude: f64,
    /// Longitude in WGS84 decimal degrees.
    #[schema(example = 18.063, minimum = -180, maximum = 180)]
    pub longitude: f64,
    /// Speed over ground in metres per second. Omitted when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = 8.3, minimum = 0)]
    pub speed: Option<f64>,
    /// Direction of travel in degrees clockwise from true north, in `[0, 360)`.
    /// Omitted when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = 45.0, minimum = 0, exclusive_maximum = 360)]
    pub heading: Option<f64>,
    /// When the server stored this position, in RFC 3339 UTC with millisecond precision.
    /// Set by the server and always present in responses.
    #[schema(format = DateTime, example = "2025-09-27T14:20:00.000Z")]
    pub timestamp: Option<String>,
}

/// A message broadcast to websocket clients: either a location update or a
/// notification that a user's location was deleted.
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    /// A user posted a new position. All `UserCoords` fields sit next to `type`.
    Update(UserCoords),
    /// A user was deleted.
    Delete {
        /// The normalized name from the delete request.
        #[schema(example = "hannes")]
        name: String,
    },
}

/// A position to store for a user. The server sets the `timestamp`.
///
/// Unknown fields are rejected, so clients cannot sneak in a `timestamp` or
/// any other unexpected property.
///
/// # Examples
///
/// Successful deserialization from JSON:
/// ```
/// use stalk_api::models::NewUserCoords;
/// let json = r#"{"name":"marin","latitude":59.3325,"longitude":18.0649}"#;
/// let dto: NewUserCoords = serde_json::from_str(json).unwrap();
/// assert_eq!(dto.name, "marin");
/// ```
///
/// Unknown fields are rejected:
/// ```
/// use stalk_api::models::NewUserCoords;
/// let json_with_unknown = r#"{"name":"m","latitude":1.0,"longitude":2.0,"timestamp":"2020-01-01T00:00:00Z"}"#;
/// let err = serde_json::from_str::<NewUserCoords>(json_with_unknown).unwrap_err();
/// let msg = err.to_string();
/// assert!(msg.contains("unknown field") && msg.contains("timestamp"));
/// ```
#[derive(Serialize, Deserialize, Debug, ToSchema)]
#[serde(deny_unknown_fields)]
// Keep the doctests above out of the published schema
#[schema(
    description = "A position to store for a user. The server sets the `timestamp`, \
    and any field not listed here is rejected."
)]
pub struct NewUserCoords {
    /// User name. Trimmed and NFC-normalized, matched case-insensitively, and must not be
    /// empty after trimming.
    #[schema(example = "marin", min_length = 1)]
    pub name: String,
    /// Latitude in WGS84 decimal degrees.
    #[schema(example = 59.3325, minimum = -90, maximum = 90)]
    pub latitude: f64,
    /// Longitude in WGS84 decimal degrees.
    #[schema(example = 18.0649, minimum = -180, maximum = 180)]
    pub longitude: f64,
    /// Speed over ground in metres per second. Omitted when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = 8.3, minimum = 0)]
    pub speed: Option<f64>,
    /// Direction of travel in degrees clockwise from true north, in `[0, 360]`; 360 is normalized to 0.
    /// Omitted when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(example = 45.0, minimum = 0, maximum = 360)]
    pub heading: Option<f64>,
}

/// Normalize a user-provided name by trimming surrounding whitespace and
/// applying Unicode NFC normalization.
///
/// This ensures that visually equivalent strings are normalized to a canonical
/// form, which is important for consistent lookups and uniqueness checks.
///
/// Examples
///
/// ```
/// use stalk_api::models::normalize_name;
///
/// // Trimming
/// assert_eq!(normalize_name("  Alice  "), "Alice");
///
/// // Unicode canonical composition ("Cafe" + combining acute accent -> "Café")
/// let decomposed = "Cafe\u{301}"; // "e" + combining acute
/// assert_eq!(normalize_name(decomposed), "Café");
/// ```
pub fn normalize_name(name: &str) -> String {
    name.trim().nfc().collect::<String>()
}

/// Validate and normalize an incoming payload into a `UserCoords` suitable for DB upsert.
///
/// Rules enforced:
/// - latitude must be finite and within [-90, 90]
/// - longitude must be finite and within [-180, 180]
/// - name must be non-empty after trimming and NFC normalization
/// - speed, if present, must be finite and non-negative
/// - heading, if present, must be finite and within [0, 360]; 360 is normalized to 0
/// - timestamp is controlled by the server
///
/// Every broken rule is reported, each as a [`FieldError`] pointing at its field.
///
/// Examples
///
/// ```
/// use stalk_api::models::{validate_and_normalize, NewUserCoords};
///
/// let input = NewUserCoords {
///     name: " Alice ".into(),
///     latitude: 10.0,
///     longitude: 20.0,
///     speed: None,
///     heading: None,
/// };
/// let out = validate_and_normalize(input).expect("valid input");
/// assert_eq!(out.name, "Alice");
/// assert!(out.timestamp.is_none());
/// ```
///
/// Speed and heading are optional and passed through, with heading normalized:
///
/// ```
/// use stalk_api::models::{validate_and_normalize, NewUserCoords};
///
/// let json = r#"{"name":"marin","latitude":59.3,"longitude":18.0,"speed":8.3,"heading":360}"#;
/// let input: NewUserCoords = serde_json::from_str(json).unwrap();
/// let out = validate_and_normalize(input).expect("valid input");
/// assert_eq!(out.speed, Some(8.3));
/// assert_eq!(out.heading, Some(0.0));
/// ```
///
/// Invalid input lists every bad field:
///
/// ```
/// use stalk_api::models::{validate_and_normalize, NewUserCoords};
///
/// let json = r#"{"name":" ","latitude":91,"longitude":18.0,"heading":400}"#;
/// let input: NewUserCoords = serde_json::from_str(json).unwrap();
/// let errors = validate_and_normalize(input).unwrap_err();
/// let pointers: Vec<_> = errors.iter().map(|e| e.pointer.as_str()).collect();
/// assert_eq!(pointers, ["#/name", "#/latitude", "#/heading"]);
/// ```
pub fn validate_and_normalize(input: NewUserCoords) -> Result<UserCoords, Vec<FieldError>> {
    // Check every field so the client hears about all problems at once, in field order
    let mut errors = Vec::new();

    let name = normalize_name(&input.name);
    if name.is_empty() {
        errors.push(FieldError::new("name", "must not be empty"));
    }
    // `contains` is false for NaN and infinities too
    if !(-90.0..=90.0).contains(&input.latitude) {
        errors.push(FieldError::new("latitude", "must be between -90 and 90"));
    }
    if !(-180.0..=180.0).contains(&input.longitude) {
        errors.push(FieldError::new("longitude", "must be between -180 and 180"));
    }
    if let Some(speed) = input.speed
        && (!speed.is_finite() || speed < 0.0)
    {
        errors.push(FieldError::new(
            "speed",
            "must be a finite, non-negative number of m/s",
        ));
    }
    if let Some(heading) = input.heading
        && !(0.0..=360.0).contains(&heading)
    {
        errors.push(FieldError::new("heading", "must be between 0 and 360"));
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // 360 is north again; `== 0.0` also folds -0.0 into 0.0, keeping output within [0, 360)
    let heading = input
        .heading
        .map(|h| if h == 0.0 || h == 360.0 { 0.0 } else { h });

    Ok(UserCoords {
        name,
        latitude: input.latitude,
        longitude: input.longitude,
        speed: input.speed,
        heading,
        timestamp: None, // server controls timestamp
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_nfc() {
        let name = "  Café  ";
        let normalized = normalize_name(name);
        assert_eq!(normalized, "Café");
    }

    #[test]
    fn validate_accepts_valid_input() {
        let input = NewUserCoords {
            name: "Alice".into(),
            latitude: 10.0,
            longitude: -20.0,
            speed: None,
            heading: None,
        };
        let out = validate_and_normalize(input).unwrap();
        assert_eq!(out.name, "Alice");
        assert_eq!(out.latitude, 10.0);
        assert_eq!(out.longitude, -20.0);
        assert!(out.timestamp.is_none());
    }

    #[test]
    fn validate_rejects_non_finite() {
        let input = NewUserCoords {
            name: "Bob".into(),
            latitude: f64::NAN,
            longitude: 0.0,
            speed: None,
            heading: None,
        };
        assert!(validate_and_normalize(input).is_err());
        let input = NewUserCoords {
            name: "Bob".into(),
            latitude: 0.0,
            longitude: f64::INFINITY,
            speed: None,
            heading: None,
        };
        assert!(validate_and_normalize(input).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range() {
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: 91.0,
                longitude: 0.0,
                speed: None,
                heading: None
            })
            .is_err()
        );
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: -91.0,
                longitude: 0.0,
                speed: None,
                heading: None
            })
            .is_err()
        );
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: 0.0,
                longitude: 181.0,
                speed: None,
                heading: None
            })
            .is_err()
        );
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: 0.0,
                longitude: -181.0,
                speed: None,
                heading: None
            })
            .is_err()
        );
    }

    #[test]
    fn validate_rejects_empty_name_after_trim() {
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "   ".into(),
                latitude: 0.0,
                longitude: 0.0,
                speed: None,
                heading: None
            })
            .is_err()
        );
    }

    fn with_motion(speed: Option<f64>, heading: Option<f64>) -> NewUserCoords {
        NewUserCoords {
            name: "Mover".into(),
            latitude: 59.0,
            longitude: 18.0,
            speed,
            heading,
        }
    }

    #[test]
    fn validate_passes_motion_through() {
        let out = validate_and_normalize(with_motion(Some(8.3), Some(45.0))).unwrap();
        assert_eq!(out.speed, Some(8.3));
        assert_eq!(out.heading, Some(45.0));

        let out = validate_and_normalize(with_motion(None, None)).unwrap();
        assert_eq!(out.speed, None);
        assert_eq!(out.heading, None);

        // Bounds are inclusive; zero speed means "standing still", not "unknown"
        let out = validate_and_normalize(with_motion(Some(0.0), Some(0.0))).unwrap();
        assert_eq!(out.speed, Some(0.0));
        assert_eq!(out.heading, Some(0.0));
    }

    #[test]
    fn validate_reports_every_invalid_field() {
        let errors = validate_and_normalize(NewUserCoords {
            name: "  ".into(),
            latitude: 91.0,
            longitude: f64::NAN,
            speed: Some(-1.0),
            heading: Some(361.0),
        })
        .unwrap_err();
        let pointers: Vec<_> = errors.iter().map(|e| e.pointer.as_str()).collect();
        assert_eq!(
            pointers,
            [
                "#/name",
                "#/latitude",
                "#/longitude",
                "#/speed",
                "#/heading"
            ]
        );
        assert_eq!(errors[1].detail, "must be between -90 and 90");
    }

    #[test]
    fn validate_normalizes_full_turn_heading() {
        let out = validate_and_normalize(with_motion(None, Some(360.0))).unwrap();
        assert_eq!(out.heading, Some(0.0));
    }

    #[test]
    fn validate_normalizes_negative_zero_heading() {
        let out = validate_and_normalize(with_motion(None, Some(-0.0))).unwrap();
        assert!(out.heading.unwrap().is_sign_positive());
        assert_eq!(serde_json::to_value(&out).unwrap()["heading"], 0.0);
    }

    #[test]
    fn validate_rejects_invalid_speed() {
        for speed in [-0.1, f64::NAN, f64::INFINITY] {
            assert!(
                validate_and_normalize(with_motion(Some(speed), None)).is_err(),
                "speed {speed} should be rejected"
            );
        }
    }

    #[test]
    fn validate_rejects_invalid_heading() {
        for heading in [-0.1, 360.1, f64::NAN, f64::NEG_INFINITY] {
            assert!(
                validate_and_normalize(with_motion(None, Some(heading))).is_err(),
                "heading {heading} should be rejected"
            );
        }
    }

    #[test]
    fn motion_fields_are_optional_and_nullable_in_json() {
        let absent: NewUserCoords =
            serde_json::from_str(r#"{"name":"a","latitude":1.0,"longitude":2.0}"#).unwrap();
        assert_eq!((absent.speed, absent.heading), (None, None));

        let null: NewUserCoords = serde_json::from_str(
            r#"{"name":"a","latitude":1.0,"longitude":2.0,"speed":null,"heading":null}"#,
        )
        .unwrap();
        assert_eq!((null.speed, null.heading), (None, None));

        let present: NewUserCoords = serde_json::from_str(
            r#"{"name":"a","latitude":1.0,"longitude":2.0,"speed":3.5,"heading":270}"#,
        )
        .unwrap();
        assert_eq!((present.speed, present.heading), (Some(3.5), Some(270.0)));

        // Still strict about anything else
        assert!(
            serde_json::from_str::<NewUserCoords>(
                r#"{"name":"a","latitude":1.0,"longitude":2.0,"velocity":3.5}"#
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_motion_is_omitted_when_serialized() {
        let coords = UserCoords {
            name: "a".into(),
            latitude: 1.0,
            longitude: 2.0,
            speed: None,
            heading: None,
            timestamp: None,
        };
        let json = serde_json::to_value(&coords).unwrap();
        assert!(json.get("speed").is_none());
        assert!(json.get("heading").is_none());

        let coords = UserCoords {
            speed: Some(8.3),
            heading: Some(45.0),
            ..coords
        };
        let json = serde_json::to_value(&coords).unwrap();
        assert_eq!(json["speed"], 8.3);
        assert_eq!(json["heading"], 45.0);
    }
}
