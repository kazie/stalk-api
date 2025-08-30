pub mod db;
pub mod models;
pub mod routes;

use sqlx::{Pool, Sqlite};

// Database connection pool and configuration shared across handlers
pub struct AppState {
    pub db: Pool<Sqlite>,
    pub auth_token: String,
}
