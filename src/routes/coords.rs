use crate::AppState;
use crate::db::{
    delete_user_by_name, get_all_coords_time_limited, get_specific_user_coords, upsert_coords,
};
use crate::models::UserCoords;
use crate::models::WsEvent;
use crate::models::normalize_name;
use crate::models::{NewUserCoords, validate_and_normalize};
use crate::problem::{
    ApiError, ProblemDetails, ServerErrorResponse, UnauthorizedResponse, UserNotFoundResponse,
};
use crate::routes::UserPath;
use actix_web::HttpResponse;
use actix_web::http::header::LOCATION;
use actix_web::web::{Data, Json, Path};
use log::debug;

/// Create or update a user's position
///
/// Stores the given position as the user's latest one and returns it with the server-assigned
/// `timestamp`. If no user with this name exists, it is created and the response is `201` with a
/// `Location` header. Otherwise it is replaced and the response is `200`.
///
/// The whole position is replaced, so leaving out `speed` or `heading` clears any previous
/// value. The name is trimmed and NFC-normalized and is matched case-insensitively.
///
/// Every successful call is pushed as an `update` event to WebSocket subscribers.
///
/// Validation (any failure returns `400` listing every invalid field in `errors`):
/// - `name` is non-empty after trimming
/// - `latitude` is between -90 and 90, `longitude` between -180 and 180
/// - `speed`, if present, is a finite number >= 0
/// - `heading`, if present, is between 0 and 360, and 360 is stored as 0
/// - no other fields, so a client-sent `timestamp` is rejected
#[utoipa::path(
    post,
    path = "/api/coords",
    request_body(
        content = NewUserCoords,
        description = "The user's current position. `speed` and `heading` are optional.",
        example = json!({
            "name": "kazie",
            "latitude": 59.3272,
            "longitude": 18.1071,
            "speed": 8.3,
            "heading": 45.0
        })
    ),
    responses(
        (status = 200, description = "Existing user's position was replaced", body = UserCoords),
        (status = 201, description = "New user was created", body = UserCoords, headers(
            ("Location" = String, description = "Path of the new user, e.g. `/api/coords/kazie`")
        )),
        (status = 400, description = "Invalid field values (listed in `errors`), malformed JSON, \
            or missing or unknown fields", body = ProblemDetails, content_type = "application/problem+json",
            example = json!({
                "type": "about:blank",
                "title": "Bad Request",
                "status": 400,
                "detail": "2 fields are invalid",
                "instance": "/api/coords",
                "errors": [
                    { "pointer": "#/latitude", "detail": "must be between -90 and 90" },
                    { "pointer": "#/heading", "detail": "must be between 0 and 360" }
                ]
            })),
        (status = 401, response = UnauthorizedResponse),
        (status = 413, description = "Request body is too large", body = ProblemDetails,
            content_type = "application/problem+json"),
        (status = 415, description = "Request body is not `Content-Type: application/json`",
            body = ProblemDetails, content_type = "application/problem+json"),
        (status = 500, response = ServerErrorResponse)
    ),
    security(("ApiToken" = [])),
    tag = "coords"
)]
pub async fn update_location(
    state: Data<AppState>,
    body: Json<NewUserCoords>,
) -> Result<HttpResponse, ApiError> {
    debug!("Updating user coords: {body:?}");
    let user = validate_and_normalize(body.into_inner()).map_err(ApiError::Validation)?;

    // Check if the user already exists to decide between 200 OK (update) and 201 Created (create)
    let existed = get_specific_user_coords(&state.db, &user.name)
        .await?
        .is_some();

    let coords = upsert_coords(&state.db, &user).await?;
    // Publish update to websocket subscribers; ignore if there are no receivers
    let _ = state.notifier.send(WsEvent::Update(coords.clone()));
    if existed {
        Ok(HttpResponse::Ok().json(coords))
    } else {
        let name = coords.name.clone();
        Ok(HttpResponse::Created()
            .insert_header((LOCATION, format!("/api/coords/{name}")))
            .json(coords))
    }
}

/// List recently active users
///
/// Returns the latest position of every user updated in the last 60 minutes, at most 20 users,
/// in no particular order. Users who have been quiet for longer are left out but can still be
/// fetched with `GET /api/coords/{name}`. The list is empty when nobody is active.
#[utoipa::path(
    get,
    path = "/api/coords",
    responses(
        (status = 200, description = "Positions updated in the last 60 minutes", body = [UserCoords]),
        (status = 500, response = ServerErrorResponse)
    ),
    tag = "coords"
)]
pub async fn get_locations(state: Data<AppState>) -> Result<HttpResponse, ApiError> {
    debug!("Get all user coords");
    let coords = get_all_coords_time_limited(&state.db).await?;
    Ok(HttpResponse::Ok().json(coords))
}

/// Get one user's position
///
/// Returns the user's latest position, however old it is. Use the `timestamp` to judge how
/// current it is. Unlike the list endpoint, this one has no 60-minute window.
#[utoipa::path(
    get,
    path = "/api/coords/{name}",
    params(UserPath),
    responses(
        (status = 200, description = "The user's latest position", body = UserCoords),
        (status = 404, response = UserNotFoundResponse),
        (status = 500, response = ServerErrorResponse)
    ),
    tag = "coords"
)]
pub async fn get_user(
    state: Data<AppState>,
    path: Path<UserPath>,
) -> Result<HttpResponse, ApiError> {
    let username = normalize_name(&path.name);
    debug!("Searching coords for user: {username}");

    match get_specific_user_coords(&state.db, &username).await? {
        Some(coords) => Ok(HttpResponse::Ok().json(coords)),
        None => Err(user_not_found(&username)),
    }
}

/// Delete a user
///
/// Removes the user and their position. Every successful call is pushed to WebSocket
/// subscribers as a `delete` event that carries the normalized name from the request.
#[utoipa::path(
    delete,
    path = "/api/coords/{name}",
    params(UserPath),
    responses(
        (status = 204, description = "User deleted"),
        (status = 401, response = UnauthorizedResponse),
        (status = 404, response = UserNotFoundResponse),
        (status = 500, response = ServerErrorResponse)
    ),
    security(("ApiToken" = [])),
    tag = "coords"
)]
pub async fn delete_user(
    state: Data<AppState>,
    path: Path<UserPath>,
) -> Result<HttpResponse, ApiError> {
    let username = normalize_name(&path.name);
    debug!("Deleting user: {username}");

    if !delete_user_by_name(&state.db, &username).await? {
        return Err(user_not_found(&username));
    }
    // Notify websocket subscribers that this user's pin was deleted; ignore if there are no receivers
    let _ = state.notifier.send(WsEvent::Delete { name: username });
    Ok(HttpResponse::NoContent().finish())
}

fn user_not_found(username: &str) -> ApiError {
    ApiError::NotFound(format!("No user named '{username}'"))
}
