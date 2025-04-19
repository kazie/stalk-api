pub mod db;
pub mod models;
pub mod routes;

use sqlx::{Pool, Sqlite};

// Database connection pool will be shared across handlers
pub struct AppState {
    pub db: Pool<Sqlite>,
}
