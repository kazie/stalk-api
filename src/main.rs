use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};

// Define our item structure
#[derive(Serialize, Deserialize, Debug)]
struct UserCoords {
    name: String,
    latitude: f64,
    longitude: f64,
    timestamp: Option<String>,
}

// Database connection pool will be shared across handlers
struct AppState {
    db: Pool<Sqlite>,
}

async fn update_location(
    state: web::Data<AppState>,
    user_cords: web::Json<UserCoords>,
) -> actix_web::Result<HttpResponse> {
    debug!("Updating user coords: {:?}", user_cords);
    let result = sqlx::query_as!(
        UserCoords,
        r#"INSERT INTO user_cords (name, latitude, longitude)
        VALUES (?, ?, ?)
        ON CONFLICT(name) DO UPDATE SET
            latitude = excluded.latitude,
            longitude = excluded.longitude,
            timestamp = datetime('now')
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
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        error!("Database error: {:?}", e);
        actix_web::error::ErrorInternalServerError(e)
    })?;

    Ok(HttpResponse::Ok().json(result))
}

// Handler for getting all items
async fn let_locations(state: web::Data<AppState>) -> impl Responder {
    debug!("Get all user coords");
    let result = sqlx::query_as!(
        UserCoords,
        r#"
        SELECT 
            name as "name!: String",
            latitude as "latitude!: f64",
            longitude as "longitude!: f64",
            timestamp as "timestamp!: String"
        FROM user_cords
        "#
    )
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(coords) => HttpResponse::Ok().json(coords),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// Handler for getting a single item by ID
async fn get_user(state: web::Data<AppState>, name: web::Path<String>) -> impl Responder {
    let path_name = name.into_inner();
    debug!("Searching coords for user: {}", path_name);
    let result = sqlx::query_as!(
        UserCoords,
        r#"
        SELECT 
            name as "name!: String",
            latitude as "latitude!: f64",
            longitude as "longitude!: f64",
            timestamp as "timestamp!: String"
        FROM user_cords
        WHERE name = ?
        "#,
        path_name
    )
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(coords)) => HttpResponse::Ok().json(coords),
        Ok(None) => HttpResponse::NotFound().json("Item not found"),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize with default info level if RUST_LOG is not set
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // Set up database connection pool
    let database_url = "sqlite:coords.sqlite";
    info!("Connecting to database: {}", database_url);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .expect("Failed to create pool");
    info!("Connected to database");

    let bind_addr = "127.0.0.1:8080";
    info!("Starting server on {}", bind_addr);
    // Start HTTP server
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState { db: pool.clone() }))
            .service(
                web::scope("/api")
                    .route("/coords", web::post().to(update_location))
                    .route("/coords", web::get().to(let_locations))
                    .route("/coords/{name}", web::get().to(get_user)),
            )
    })
    .bind(bind_addr)?
    .run()
    .await
}
