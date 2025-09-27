pub mod db;
pub mod models;
pub mod openapi;
pub mod routes;
pub mod utils;

use actix_web::web;
use actix_web_httpauth::middleware::HttpAuthentication;
use sqlx::{Pool, Sqlite};
use utoipa::OpenApi;
use utoipa_rapidoc::RapiDoc;
use utoipa_swagger_ui::SwaggerUi;

// Database connection pool and configuration shared across handlers
pub struct AppState {
    pub db: Pool<Sqlite>,
    pub auth_token: String,
    pub notifier: tokio::sync::broadcast::Sender<crate::models::UserCoords>,
}

// Reuse the same bearer validator from main in both prod and tests
pub async fn validator(
    req: actix_web::dev::ServiceRequest,
    credentials: actix_web_httpauth::extractors::bearer::BearerAuth,
) -> Result<actix_web::dev::ServiceRequest, (actix_web::Error, actix_web::dev::ServiceRequest)> {
    if let Some(state) = req.app_data::<web::Data<AppState>>() {
        if credentials.token() == state.auth_token {
            Ok(req)
        } else {
            Err((actix_web::error::ErrorUnauthorized("invalid token"), req))
        }
    } else {
        Err((
            actix_web::error::ErrorUnauthorized("authentication not configured"),
            req,
        ))
    }
}

// Configure the Actix Web services exactly like main.rs so tests exercise the real paths
pub fn configure_api(cfg: &mut web::ServiceConfig) {
    let auth = HttpAuthentication::bearer(validator);
    cfg.service(
        actix_web::web::resource("/health").route(actix_web::web::get().to(crate::routes::health)),
    )
    .service(crate::routes::ws_coords)
    .service(crate::routes::ws_coords_user)
    .service(
        web::scope("/api")
            .service(
                web::resource("/coords")
                    .route(
                        web::post()
                            .to(crate::routes::update_location)
                            .wrap(auth.clone()),
                    )
                    .route(web::get().to(crate::routes::get_locations)),
            )
            .service(
                web::resource("/coords/{name}")
                    .route(web::get().to(crate::routes::get_user))
                    .route(web::delete().to(crate::routes::delete_user).wrap(auth)),
            )
            .service(
                web::resource("/openapi.json").route(web::get().to(|| async {
                    actix_web::HttpResponse::Ok().json(crate::openapi::ApiDoc::openapi())
                })),
            )
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api/openapi.json", crate::openapi::ApiDoc::openapi()),
            )
            .service(RapiDoc::new("/api/openapi.json").path("/rapidoc")),
    );
}
