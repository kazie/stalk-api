use log::{error, info};
use sqlx::{Pool, Sqlite};

pub fn check_and_create_db_file(db_file: &str) {
    let db_path = std::path::Path::new(db_file);
    if db_path.exists() {
        error!(
            "Database file '{}' already exists. Migration is only for new databases.",
            db_file
        );
        std::process::exit(1);
    }
    // Create empty database file
    let _ = std::fs::File::create(db_file).map_err(|e| {
        error!("Failed to create database file: {}", e);
        std::process::exit(1);
    });
    info!("Created empty database file '{}'", db_file);
}

pub async fn migrate(db_file: &str, db_pool: &Pool<Sqlite>) {
    let result = sqlx::migrate!("./migrations").run(db_pool).await;
    match result {
        Ok(_) => {
            info!("Database migrated to file '{}'", db_file);
            std::process::exit(0);
        }
        Err(e) => {
            error!("Database migration failed: {}", e);
            std::process::exit(1);
        }
    }
}
