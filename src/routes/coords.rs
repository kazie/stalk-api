use crate::AppState;
use crate::db::{
    delete_user_by_name, get_all_coords_time_limited, get_specific_user_coords, upsert_coords,
};
use crate::models::UserCoords;
use crate::models::normalize_name;
use crate::models::{NewUserCoords, validate_and_normalize};
use actix_web::http::header::LOCATION;
use actix_web::web::{Data, Json, Path};
use actix_web::{HttpResponse, Responder};
use log::{debug, error};

/// Create or update a user's coordinates
#[utoipa::path(
    post,
    path = "/api/coords",
    request_body = NewUserCoords,
    responses(
        (status = 200, description = "Updated existing user coordinates", body = UserCoords),
        (status = 201, description = "Created new user coordinates", body = UserCoords, headers(
            ("Location" = String, description = "Resource location")
        )),
        (status = 400, description = "Invalid input"),
        (status = 500, description = "Internal server error")
    ),
    security(("ApiToken" = [])),
    tag = "coords"
)]
pub async fn update_location(state: Data<AppState>, body: Json<NewUserCoords>) -> impl Responder {
    debug!("Updating user coords: {body:?}");
    let validated: Result<UserCoords, String> = validate_and_normalize(body.into_inner());
    let user = match validated {
        Ok(u) => u,
        Err(msg) => return HttpResponse::BadRequest().body(msg),
    };

    // Check if the user already exists to decide between 200 OK (update) and 201 Created (create)
    let existed = match get_specific_user_coords(&state.db, &user.name).await {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(e) => {
            error!("Database error during existence check: {e:?}");
            return HttpResponse::InternalServerError().body(e.to_string());
        }
    };

    let result = upsert_coords(&state.db, &user).await;

    match result {
        Ok(coords) => {
            if existed {
                HttpResponse::Ok().json(coords)
            } else {
                let name = coords.name.clone();
                HttpResponse::Created()
                    .insert_header((LOCATION, format!("/api/coords/{name}")))
                    .json(coords)
            }
        }
        Err(e) => {
            error!("Database error: {e:?}");
            HttpResponse::InternalServerError().body(e.to_string())
        }
    }
}

// Handler for getting all items
#[utoipa::path(
    get,
    path = "/api/coords",
    responses(
        (status = 200, description = "List of current user coordinates", body = [UserCoords]),
        (status = 500, description = "Internal server error")
    ),
    tag = "coords"
)]
pub async fn get_locations(state: Data<AppState>) -> impl Responder {
    debug!("Get all user coords");
    let result = get_all_coords_time_limited(&state.db).await;

    match result {
        Ok(coords) => HttpResponse::Ok().json(coords),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// Handler for getting a single item by ID
#[utoipa::path(
    get,
    path = "/api/coords/{name}",
    params(
        ("name" = String, Path, description = "User name")
    ),
    responses(
        (status = 200, description = "User coordinates", body = UserCoords),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "coords"
)]
pub async fn get_user(state: Data<AppState>, name: Path<String>) -> impl Responder {
    let username_raw = name.as_str();
    let username = normalize_name(username_raw);
    debug!("Searching coords for user: {username}");
    let result = get_specific_user_coords(&state.db, &username).await;

    match result {
        Ok(Some(coords)) => HttpResponse::Ok().json(coords),
        Ok(None) => HttpResponse::NotFound().json("Item not found"),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

#[utoipa::path(
    delete,
    path = "/api/coords/{name}",
    params(
        ("name" = String, Path, description = "User name")
    ),
    responses(
        (status = 204, description = "User deleted"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("ApiToken" = [])),
    tag = "coords"
)]
pub async fn delete_user(state: Data<AppState>, name: Path<String>) -> impl Responder {
    let username_raw = name.as_str();
    let username = normalize_name(username_raw);
    debug!("Deleting user: {username}");

    match delete_user_by_name(&state.db, &username).await {
        Ok(true) => HttpResponse::NoContent().finish(),
        Ok(false) => HttpResponse::NotFound().json("Item not found"),
        Err(e) => {
            error!("Database error during delete: {e:?}");
            HttpResponse::InternalServerError().body(e.to_string())
        }
    }
}
