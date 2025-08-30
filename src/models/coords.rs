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
