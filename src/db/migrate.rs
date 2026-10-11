use log::{error, info};
use sqlx::migrate::MigrateError;
use sqlx::{Pool, Sqlite};

pub fn check_and_create_db_file(db_file: &str) {
    let db_path = std::path::Path::new(db_file);
    if db_path.exists() {
        error!("Database file '{db_file}' already exists. Migration is only for new databases.");
        std::process::exit(1);
    }
    // Create empty database file
    let _ = std::fs::File::create(db_file).map_err(|e| {
        error!("Failed to create database file: {e}");
        std::process::exit(1);
    });
    info!("Created empty database file '{db_file}'");
}

/// Apply any migrations not yet recorded in the database. Already applied
/// migrations are skipped, so this is safe to run on every startup.
pub async fn run_pending_migrations(db_pool: &Pool<Sqlite>) -> Result<(), MigrateError> {
    sqlx::migrate!("./migrations").run(db_pool).await
}
