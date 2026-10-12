pub mod db;
pub mod models;
pub mod openapi;
pub mod problem;
pub mod routes;
pub mod utils;

use actix_web::web;
use actix_web_httpauth::middleware::HttpAuthentication;
use problem::ApiError;
use sqlx::{Pool, Sqlite};
use utoipa::OpenApi;
use utoipa_rapidoc::RapiDoc;
use utoipa_swagger_ui::SwaggerUi;

// Database connection pool and configuration shared across handlers
pub struct AppState {
    pub db: Pool<Sqlite>,
    pub auth_token: String,
    pub notifier: tokio::sync::broadcast::Sender<crate::models::WsEvent>,
}

// Reuse the same bearer validator from main in both prod and tests. Credentials are optional so a
// missing header reaches us and gets the same problem response as a wrong token.
pub async fn validator(
    req: actix_web::dev::ServiceRequest,
    credentials: Option<actix_web_httpauth::extractors::bearer::BearerAuth>,
) -> Result<actix_web::dev::ServiceRequest, (actix_web::Error, actix_web::dev::ServiceRequest)> {
    let Some(credentials) = credentials else {
        return Err((ApiError::MissingToken.into(), req));
    };
    match req.app_data::<web::Data<AppState>>() {
        Some(state) if credentials.token() == state.auth_token => Ok(req),
        Some(_) => Err((ApiError::InvalidToken.into(), req)),
        None => {
            log::error!("AppState missing, cannot check bearer token");
            Err((ApiError::Internal.into(), req))
        }
    }
}

// Configure the Actix Web services exactly like main.rs so tests exercise the real paths
pub fn configure_api(cfg: &mut web::ServiceConfig) {
    let auth = HttpAuthentication::with_fn(validator);
    let api = web::scope("/api")
        .service(web::resource("/health").route(web::get().to(crate::routes::health)))
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
        .service(RapiDoc::new("/api/openapi.json").path("/rapidoc"));
    let ws = web::scope("/ws")
        .service(crate::routes::ws_coords)
        .service(crate::routes::ws_coords_user);

    // Every error is RFC 9457 problem JSON. The root scope matches all paths, so its middleware
    // also renders unmatched paths (the app default service) and errors actix produces itself.
    cfg.app_data(web::JsonConfig::default().error_handler(problem::json_error_handler))
        .default_service(web::to(problem::not_found))
        .service(
            web::scope("")
                .wrap(problem::problem_details())
                .service(ws)
                .service(api),
        );
}
