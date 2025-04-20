use crate::models::UserCoords;
use sqlx::{Error, Pool, Sqlite};

pub async fn upsert_coords(
    db: &Pool<Sqlite>,
    user_cords: &UserCoords,
) -> Result<UserCoords, Error> {
    sqlx::query_as!(
        UserCoords,
        r#"
        INSERT INTO user_cords (name, latitude, longitude)
        VALUES (?, ?, ?)
        ON CONFLICT(name) DO UPDATE SET
            latitude = excluded.latitude,
            longitude = excluded.longitude,
            timestamp = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        RETURNING
            name as "name!: String",
            latitude as "latitude!: f64",
            longitude as "longitude!: f64",
            timestamp as "timestamp!: String"
        "#,
        user_cords.name,
        user_cords.latitude,
        user_cords.longitude
    )
    .fetch_one(db)
    .await
}

pub async fn get_all_cords(db: &Pool<Sqlite>) -> Result<Vec<UserCoords>, Error> {
    sqlx::query_as!(
        UserCoords,
        r#"
        SELECT
            name as "name!: String",
            latitude as "latitude!: f64",
            longitude as "longitude!: f64",
            timestamp as "timestamp!: String"
        FROM user_cords
        LIMIT 20; -- If more users, create streaming or something, lol
        "#
    )
    .fetch_all(db)
    .await
}

pub async fn get_all_cords_time_limited(db: &Pool<Sqlite>) -> Result<Vec<UserCoords>, Error> {
    sqlx::query_as!(
        UserCoords,
        r#"
        SELECT
            name as "name!: String",
            latitude as "latitude!: f64",
            longitude as "longitude!: f64",
            timestamp as "timestamp!: String"
        FROM user_cords
        WHERE timestamp > strftime('%Y-%m-%dT%H:%M:%fZ', datetime('now', '-60 minutes'))
        LIMIT 20; -- If more users, create streaming or something, lol
        "#
    )
    .fetch_all(db)
    .await
}

pub async fn get_specific_user_coords(
    db: &Pool<Sqlite>,
    username: &str,
) -> Result<Option<UserCoords>, Error> {
    sqlx::query_as!(
        UserCoords,
        r#"
        SELECT
            name as "name!: String",
            latitude as "latitude!: f64",
            longitude as "longitude!: f64",
            timestamp as "timestamp!: String"
        FROM user_cords
        WHERE name = ?  -- uniq index, so limited to 1 answer
        "#,
        username
    )
    .fetch_optional(db)
    .await
}

pub async fn get_specific_user_coords_time_limited(
    db: &Pool<Sqlite>,
    username: &str,
) -> Result<Option<UserCoords>, Error> {
    sqlx::query_as!(
        UserCoords,
        r#"
        SELECT
            name as "name!: String",
            latitude as "latitude!: f64",
            longitude as "longitude!: f64",
            timestamp as "timestamp!: String"
        FROM user_cords
        WHERE name = ?  -- uniq index, so limited to 1 answer
        AND timestamp > strftime('%Y-%m-%dT%H:%M:%fZ', datetime('now', '-60 minutes'))
        "#,
        username
    )
    .fetch_optional(db)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::MigrateDatabase;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_test_db() -> Pool<Sqlite> {
        // Create and connect to an in-memory database
        let db_url = "sqlite::memory:";

        if !Sqlite::database_exists(db_url).await.unwrap_or(false) {
            Sqlite::create_database(db_url)
                .await
                .expect("Failed to create test database");
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(db_url)
            .await
            .expect("Failed to create test database pool");

        // Run migrations from the migrations directory
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        pool
    }

    #[sqlx::test]
    async fn test_get_specific_user_coords_empty_db() {
        let pool = setup_test_db().await;
        let result = get_specific_user_coords_time_limited(&pool, "nonexistent").await;
        assert!(result.unwrap().is_none());
    }

    #[sqlx::test]
    async fn test_get_specific_user_coords_with_data() {
        let pool = setup_test_db().await;

        // Insert test data
        let test_user = UserCoords {
            name: "testuser".to_string(),
            latitude: 59.32721,
            longitude: 18.10710,
            timestamp: None, // No insertions of timestamp through API
        };

        // Insert using your upsert function
        let inserted = upsert_coords(&pool, &test_user).await.unwrap();
        assert_eq!(inserted.name, test_user.name);
        assert_eq!(inserted.latitude, test_user.latitude);
        assert_eq!(inserted.longitude, test_user.longitude);

        // Test retrieval
        let retrieved = get_specific_user_coords_time_limited(&pool, &test_user.name)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(retrieved.name, test_user.name);
        assert_eq!(retrieved.latitude, test_user.latitude);
        assert_eq!(retrieved.longitude, test_user.longitude);
    }

    #[sqlx::test]
    async fn test_expired_coords_not_returned() {
        let pool = setup_test_db().await;

        // Directly insert an expired record using SQL
        sqlx::query(
            r#"
            INSERT INTO user_cords (name, latitude, longitude, timestamp)
            VALUES (?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', datetime('now', '-2 day')))
            "#,
        )
        .bind("olduser")
        .bind(59.32721)
        .bind(18.10710)
        .execute(&pool)
        .await
        .unwrap();

        // Should return None because the record is older than 1 day
        let result = get_specific_user_coords_time_limited(&pool, "olduser")
            .await
            .unwrap();
        assert!(result.is_none());
        let all_find = get_specific_user_coords(&pool, "olduser").await.unwrap();
        assert!(!all_find.is_none());
    }
}
