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
pub struct ApiDoc;
