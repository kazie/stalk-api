use actix_web::dev::ServiceRequest;
use actix_web::error::ErrorUnauthorized;
use actix_web::{App, Error, HttpServer, web};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::middleware::HttpAuthentication;
use clap::Parser;
use log::info;
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::AppState;
use stalk_api::db::{check_and_create_db_file, migrate};
use stalk_api::routes::{get_locations, get_user, health, update_location};
use std::env;

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

async fn validator(
    req: ServiceRequest,
    credentials: BearerAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    // Read the token from application state loaded at startup
    if let Some(state) = req.app_data::<web::Data<AppState>>() {
        if credentials.token() == state.auth_token {
            Ok(req)
        } else {
            Err((ErrorUnauthorized("invalid token"), req))
        }
    } else {
        Err((ErrorUnauthorized("authentication not configured"), req))
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
    let database_url = format!("sqlite:{db_file}");
    if args.migrate {
        check_and_create_db_file(db_file);
    }

    info!("Connecting to database: {database_url}");
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
    let port = args.port;
    let bind_addr = format!("{host}:{port}");
    info!("Starting server on {bind_addr}");
    // Load API token once at startup
    let token = match env::var("API_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            eprintln!(
                "Configuration error: API_TOKEN is not set or is empty. Please set the environment variable and restart the service."
            );
            std::process::exit(1);
        }
    };
    // Start HTTP server and set up graceful shutdown
    let pool_clone = pool.clone();
    let server = HttpServer::new(move || {
        let auth = HttpAuthentication::bearer(validator);
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool_clone.clone(),
                auth_token: token.clone(),
            }))
            // Public health endpoint (no auth)
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth))
                            .route(web::get().to(get_locations)),
                    )
                    .service(web::resource("/coords/{name}").route(web::get().to(get_user))),
            )
    })
    .shutdown_timeout(5)
    .disable_signals()
    .bind(bind_addr)?
    .run();

    let handle = server.handle();

    // Signal handling tasks to stop the server gracefully
    {
        let handle = handle.clone();
        actix_web::rt::spawn(async move {
            let _ = actix_web::rt::signal::ctrl_c().await;
            info!("Shutdown signal (Ctrl+C) received, stopping server gracefully...");
            handle.stop(true).await;
        });
    }

    #[cfg(unix)]
    {
        use actix_web::rt::signal::unix::{signal, SignalKind};
        let handle = handle.clone();
        actix_web::rt::spawn(async move {
            match signal(SignalKind::terminate()) {
                Ok(mut sigterm) => {
                    let _ = sigterm.recv().await;
                    info!("SIGTERM received, stopping server gracefully...");
                    handle.stop(true).await;
                }
                Err(e) => {
                    info!("Failed to install SIGTERM handler: {e}");
                }
            }
        });
    }

    // Wait for server to exit
    let result = server.await;

    // Close the database pool after server stops
    pool.close().await;

    result
}
