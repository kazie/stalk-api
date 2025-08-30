use actix_web::http::{Method, StatusCode, header};
use actix_web::{App, HttpServer, test, web};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::middleware::HttpAuthentication;
use awc::Client;
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::AppState;
use stalk_api::routes::{delete_user, get_locations, get_user, health, update_location};
use std::net::TcpListener;

// Local auth validator mirroring main.rs behavior (checks AppState.auth_token)
async fn validator(
    req: actix_web::dev::ServiceRequest,
    credentials: BearerAuth,
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

async fn build_pool_and_token() -> (sqlx::Pool<sqlx::Sqlite>, String) {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory pool");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");

    (pool, "test-token".to_string())
}

#[actix_web::test]
async fn health_returns_ok_json() {
    let (pool, token) = build_pool_and_token().await;
    let auth = HttpAuthentication::bearer(validator);

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token,
            }))
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth.clone()))
                            .route(web::get().to(get_locations)),
                    )
                    .service(web::resource("/coords/{name}").route(web::get().to(get_user))),
            ),
    )
    .await;

    let req = test::TestRequest::get().uri("/health").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["status"], "ok");
}

#[actix_web::test]
async fn post_coords_requires_auth() {
    let (pool, token) = build_pool_and_token().await;
    let auth = HttpAuthentication::bearer(validator);

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token,
            }))
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth.clone()))
                            .route(web::get().to(get_locations)),
                    )
                    .service(web::resource("/coords/{name}").route(web::get().to(get_user))),
            ),
    )
    .await;

    let payload = serde_json::json!({
        "name": "alice",
        "latitude": 10.0,
        "longitude": 20.0
    });

    // No auth -> 401
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Wrong token -> 401
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer wrong"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Correct token -> 201 Created
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert!(resp.headers().contains_key(header::LOCATION));

    // Second POST (update) -> 200 OK
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[actix_web::test]
async fn get_user_happy_path_and_not_found_and_case_insensitive() {
    let (pool, token) = build_pool_and_token().await;
    let auth = HttpAuthentication::bearer(validator);

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token,
            }))
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth.clone()))
                            .route(web::get().to(get_locations)),
                    )
                    .service(web::resource("/coords/{name}").route(web::get().to(get_user))),
            ),
    )
    .await;

    // Not found initially
    let req = test::TestRequest::get().uri("/api/coords/bob").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Insert via POST
    let payload = serde_json::json!({
        "name": "Bob",
        "latitude": 55.5,
        "longitude": 44.4
    });
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert!(matches!(
        resp.status(),
        StatusCode::CREATED | StatusCode::OK
    ));

    // Fetch with different case
    let req = test::TestRequest::get().uri("/api/coords/bOb").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["name"], "Bob");
}

#[actix_web::test]
async fn options_does_not_500() {
    let (pool, token) = build_pool_and_token().await;
    let auth = HttpAuthentication::bearer(validator);

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token,
            }))
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth.clone()))
                            .route(web::get().to(get_locations)),
                    )
                    .service(web::resource("/coords/{name}").route(web::get().to(get_user))),
            ),
    )
    .await;

    let req = test::TestRequest::default()
        .method(Method::OPTIONS)
        .uri("/api/coords")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert!(resp.status() != StatusCode::INTERNAL_SERVER_ERROR);
}

#[actix_web::test]
async fn smoke_boot_server_and_health() {
    let (pool, token) = build_pool_and_token().await;

    // Bind to ephemeral port
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let addr = listener.local_addr().unwrap();

    // Build server
    let auth = HttpAuthentication::bearer(validator);
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token.clone(),
            }))
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth.clone()))
                            .route(web::get().to(get_locations)),
                    )
                    .service(web::resource("/coords/{name}").route(web::get().to(get_user))),
            )
    })
    .listen(listener)
    .expect("listen")
    .run();

    // Run server in background
    let _handle = actix_web::rt::spawn(server);

    // Call health endpoint using real HTTP client
    let client = Client::default();
    let url = format!("http://{addr}/health");
    let mut resp = client.get(url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[actix_web::test]
async fn delete_requires_auth() {
    let (pool, token) = build_pool_and_token().await;
    let auth = HttpAuthentication::bearer(validator);

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token,
            }))
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth.clone()))
                            .route(web::get().to(get_locations)),
                    )
                    .service(
                        web::resource("/coords/{name}")
                            .route(web::get().to(get_user))
                            .route(web::delete().to(delete_user).wrap(auth.clone())),
                    ),
            ),
    )
    .await;

    // No auth -> 401
    let req = test::TestRequest::delete()
        .uri("/api/coords/any")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Wrong token -> 401
    let req = test::TestRequest::delete()
        .uri("/api/coords/any")
        .insert_header((header::AUTHORIZATION, "Bearer wrong"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn delete_flow_create_then_delete_then_404() {
    let (pool, token) = build_pool_and_token().await;
    let auth = HttpAuthentication::bearer(validator);

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token,
            }))
            .service(web::resource("/health").route(web::get().to(health)))
            .service(
                web::scope("/api")
                    .service(
                        web::resource("/coords")
                            .route(web::post().to(update_location).wrap(auth.clone()))
                            .route(web::get().to(get_locations)),
                    )
                    .service(
                        web::resource("/coords/{name}")
                            .route(web::get().to(get_user))
                            .route(web::delete().to(delete_user).wrap(auth.clone())),
                    ),
            ),
    )
    .await;

    // Create user
    let payload = serde_json::json!({
        "name": "Charlie",
        "latitude": 12.34,
        "longitude": 56.78
    });
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert!(matches!(
        resp.status(),
        StatusCode::CREATED | StatusCode::OK
    ));

    // Ensure GET returns 200 (lowercase path, name normalization makes it match)
    let req = test::TestRequest::get()
        .uri("/api/coords/charlie")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // DELETE using different case, expect 204
    let req = test::TestRequest::delete()
        .uri("/api/coords/ChArLiE")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Now GET should be 404
    let req = test::TestRequest::get()
        .uri("/api/coords/charlie")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Deleting again should be 404
    let req = test::TestRequest::delete()
        .uri("/api/coords/charlie")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
