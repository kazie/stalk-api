use std::env;
use actix_web::{web, App, Error, HttpServer};
use actix_web::dev::ServiceRequest;
use actix_web::error::ErrorUnauthorized;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::middleware::HttpAuthentication;
use clap::Parser;
use log::info;
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::db::{check_and_create_db_file, migrate};
use stalk_api::routes::{get_locations, get_user, update_location};
use stalk_api::AppState;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Whether to bind to all interfaces (0.0.0.0) or just localhost (127.0.0.1)
    #[arg(long, default_value_t = false)]
    public: bool,

    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Whether to migrate the database with migrations, thus creating a database file
    #[arg(long, default_value_t = false)]
    migrate: bool,

    /// Database file to use
    #[arg(short, long, default_value = "coords.sqlite")]
    db_file: String,
}

async fn validator(req: ServiceRequest, credentials: BearerAuth) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    let token = env::var("API_TOKEN").expect("API_TOKEN must be set");
    if credentials.token() == token {
        Ok(req)
    } else {
        Err((ErrorUnauthorized("invalid token"), req))
    }
}


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize with default info level if RUST_LOG is not set
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = Args::parse();

    // Set up database connection pool
    let db_file = args.db_file.as_str();
    let database_url = format!("sqlite:{}", db_file);
    if args.migrate {
        check_and_create_db_file(db_file);
    }

    info!("Connecting to database: {}", database_url);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url.as_str())
        .await
        .expect("Failed to create pool");
    info!("Connected to database");
    if args.migrate {
        migrate(db_file, &pool).await
    }

    let host = if args.public { "0.0.0.0" } else { "127.0.0.1" };
    let bind_addr = format!("{}:{}", host, args.port);
    info!("Starting server on {}", bind_addr);
    // Start HTTP server
    HttpServer::new(move || {
        let auth = HttpAuthentication::bearer(validator);
        App::new()
            .app_data(web::Data::new(AppState { db: pool.clone() }))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth))
                            .route(web::get().to(get_locations))
                    )
                    .service(web::resource("/coords/{name}").route(web::get().to(get_user)))
            )

    })
    .bind(bind_addr)?
    .run()
    .await
}
