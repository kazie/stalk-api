use crate::db::{
    get_all_cords_time_limited, get_specific_user_coords, upsert_coords,
};
use crate::models::UserCoords;
use crate::AppState;
use actix_web::web::{Data, Json, Path};
use actix_web::{HttpResponse, Responder};
use log::{debug, error};

pub async fn update_location(
    state: Data<AppState>,
    user_cords: Json<UserCoords>,
) -> impl Responder {
    debug!("Updating user coords: {:?}", user_cords);
    let result = upsert_coords(&state.db, &user_cords).await;

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
    let result = get_all_cords_time_limited(&state.db).await;

    match result {
        Ok(coords) => HttpResponse::Ok().json(coords),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// Handler for getting a single item by ID
pub async fn get_user(state: Data<AppState>, name: Path<String>) -> impl Responder {
    let username = name.as_str();
    debug!("Searching coords for user: {}", username);
    let result = get_specific_user_coords(&state.db, username).await;

    match result {
        Ok(Some(coords)) => HttpResponse::Ok().json(coords),
        Ok(None) => HttpResponse::NotFound().json("Item not found"),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}
