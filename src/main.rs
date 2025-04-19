use actix_web::{web, App, HttpServer};
use clap::Parser;
use log::{error, info};
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::routes::{get_user, let_locations, update_location};
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize with default info level if RUST_LOG is not set
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args = Args::parse();

    // Set up database connection pool
    let database_url = format!("sqlite:{}", args.db_file.as_str());
    info!("Connecting to database: {}", database_url);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url.as_str())
        .await
        .expect("Failed to create pool");
    info!("Connected to database");

    if args.migrate {
        let result = sqlx::migrate!("./migrations").run(&pool).await;
        match result {
            Ok(_) => {
                info!("Database migrated to file {}", database_url);
                std::process::exit(0);
            }
            Err(e) => {
                error!("Database migration failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    let host = if args.public { "0.0.0.0" } else { "127.0.0.1" };
    let bind_addr = format!("{}:{}", host, args.port);
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
