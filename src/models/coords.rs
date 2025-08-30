use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

// Define our item structure for storage/response
#[derive(Serialize, Deserialize, Debug)]
pub struct UserCoords {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timestamp: Option<String>,
}

// Input DTO without timestamp; reject unknown fields (e.g., client-sent timestamp)
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct NewUserCoords {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

// Normalize name: trim and NFC
pub fn normalize_name(name: &str) -> String {
    name.trim().nfc().collect::<String>()
}

// Validate and normalize incoming payload into a UserCoords suitable for DB upsert
pub fn validate_and_normalize(input: NewUserCoords) -> Result<UserCoords, String> {
    // Validate latitude and longitude ranges and finiteness
    if !input.latitude.is_finite() || !input.longitude.is_finite() {
        return Err("latitude/longitude must be finite numbers".to_string());
    }
    if input.latitude < -90.0 || input.latitude > 90.0 {
        return Err("latitude must be between -90 and 90".to_string());
    }
    if input.longitude < -180.0 || input.longitude > 180.0 {
        return Err("longitude must be between -180 and 180".to_string());
    }

    // Normalize and validate name
    let name = normalize_name(&input.name);
    if name.is_empty() {
        return Err("name must be non-empty".to_string());
    }

    Ok(UserCoords {
        name,
        latitude: input.latitude,
        longitude: input.longitude,
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
        };
        assert!(validate_and_normalize(input).is_err());
        let input = NewUserCoords {
            name: "Bob".into(),
            latitude: 0.0,
            longitude: f64::INFINITY,
        };
        assert!(validate_and_normalize(input).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range() {
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: 91.0,
                longitude: 0.0
            })
            .is_err()
        );
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: -91.0,
                longitude: 0.0
            })
            .is_err()
        );
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: 0.0,
                longitude: 181.0
            })
            .is_err()
        );
        assert!(
            validate_and_normalize(NewUserCoords {
                name: "A".into(),
                latitude: 0.0,
                longitude: -181.0
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
                longitude: 0.0
            })
            .is_err()
        );
    }
}
