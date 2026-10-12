mod coords;
pub use coords::*;
mod ws;
pub use ws::*;

use actix_web::{HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Path parameters of the per-user endpoints.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct UserPath {
    /// User name. Trimmed, NFC-normalized and matched case-insensitively.
    #[param(example = "kazie")]
    pub name: String,
}

/// Health check result.
#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    /// Always `ok` when the service answers.
    #[schema(example = "ok")]
    pub status: &'static str,
}

/// Health check
///
/// Answers `200` with `{"status":"ok"}` while the HTTP server is up. It is a liveness check only
/// and does not touch the database.
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse)
    ),
    tag = "health"
)]
pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(HealthResponse { status: "ok" })
}
