use crate::db::{get_all_coords_time_limited, get_specific_user_coords, upsert_coords};
use crate::models::{validate_and_normalize, NewUserCoords};
use crate::models::UserCoords;
use crate::models::normalize_name;
use crate::AppState;
use actix_web::web::{Data, Json, Path};
use actix_web::{HttpResponse, Responder};
use log::{debug, error};

pub async fn update_location(
    state: Data<AppState>,
    body: Json<NewUserCoords>,
) -> impl Responder {
    debug!("Updating user coords: {:?}", body);
    let validated: Result<UserCoords, String> = validate_and_normalize(body.into_inner());
    let user = match validated {
        Ok(u) => u,
        Err(msg) => return HttpResponse::BadRequest().body(msg),
    };

    let result = upsert_coords(&state.db, &user).await;

    match result {
        Ok(coords) => HttpResponse::Ok().json(coords),
        Err(e) => {
            error!("Database error: {:?}", e);
            HttpResponse::InternalServerError().body(e.to_string())
        }
    }
}

// Handler for getting all items
pub async fn get_locations(state: Data<AppState>) -> impl Responder {
    debug!("Get all user coords");
    let result = get_all_coords_time_limited(&state.db).await;

    match result {
        Ok(coords) => HttpResponse::Ok().json(coords),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// Handler for getting a single item by ID
pub async fn get_user(state: Data<AppState>, name: Path<String>) -> impl Responder {
    let username_raw = name.as_str();
    let username = normalize_name(username_raw);
    debug!("Searching coords for user: {}", username);
    let result = get_specific_user_coords(&state.db, &username).await;

    match result {
        Ok(Some(coords)) => HttpResponse::Ok().json(coords),
        Ok(None) => HttpResponse::NotFound().json("Item not found"),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}
