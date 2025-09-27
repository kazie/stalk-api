use crate::models::{NewUserCoords, UserCoords};
use crate::routes::HealthResponse;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "ApiToken",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::health,
        crate::routes::get_locations,
        crate::routes::get_user,
        crate::routes::update_location,
        crate::routes::delete_user,
        crate::routes::ws_coords,
        crate::routes::ws_coords_user,
    ),
    components(
        schemas(UserCoords, NewUserCoords, HealthResponse)
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Health check"),
        (name = "coords", description = "Coordinates API")
    )
)]
/// OpenAPI document for the service.
///
/// # Examples
///
/// The generated OpenAPI document contains the REST and WebSocket paths:
/// ```
/// use utoipa::OpenApi;
/// use stalk_api::openapi::ApiDoc;
/// let json = serde_json::to_string(&ApiDoc::openapi()).unwrap();
/// assert!(json.contains("/api/coords"));
/// assert!(json.contains("/ws/coords"));
/// ```
pub struct ApiDoc;
