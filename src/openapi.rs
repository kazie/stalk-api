use crate::models::{NewUserCoords, UserCoords, WsEvent};
use crate::problem::{
    FieldError, ProblemDetails, ServerErrorResponse, UnauthorizedResponse, UserNotFoundResponse,
};
use crate::routes::HealthResponse;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "ApiToken",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "The server's `API_TOKEN`, sent as `Authorization: Bearer <API_TOKEN>`. \
                         Required for creating, updating and deleting positions.",
                    ))
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(title = "stalk-api", description = include_str!("openapi.md")),
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
        schemas(UserCoords, NewUserCoords, HealthResponse, WsEvent, ProblemDetails, FieldError),
        responses(UnauthorizedResponse, UserNotFoundResponse, ServerErrorResponse)
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness check for load balancers and monitoring."),
        (name = "coords", description = "Read and write user positions over REST. \
            Writes need the `ApiToken` bearer token, reads are public."),
        (name = "websocket", description = include_str!("openapi_websocket.md"))
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
