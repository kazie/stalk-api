use serde::{Deserialize, Serialize};

// Define our item structure
#[derive(Serialize, Deserialize, Debug)]
pub struct UserCoords {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timestamp: Option<String>,
}
