use actix_web::http::{Method, StatusCode, header};
use actix_web::{App, HttpServer, test, web};
use awc::Client;
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::problem::PROBLEM_JSON;
use stalk_api::{AppState, configure_api};
use std::net::TcpListener;

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

fn make_state(db: sqlx::Pool<sqlx::Sqlite>, token: String) -> web::Data<AppState> {
    let (coords_update_sender, _unused_receiver) = tokio::sync::broadcast::channel(16);
    web::Data::new(AppState {
        db,
        auth_token: token,
        notifier: coords_update_sender,
    })
}

async fn init_app() -> impl actix_web::dev::Service<
    actix_http::Request,
    Response = actix_web::dev::ServiceResponse,
    Error = actix_web::Error,
> {
    let (pool, token) = build_pool_and_token().await;
    test::init_service(
        App::new()
            .app_data(make_state(pool, token))
            .configure(configure_api),
    )
    .await
}

/// Asserts the response is an RFC 9457 problem with the given status, for the given path, and
/// returns its body.
async fn read_problem(
    resp: actix_web::dev::ServiceResponse,
    status: StatusCode,
    instance: &str,
) -> serde_json::Value {
    assert_eq!(resp.status(), status);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        PROBLEM_JSON
    );
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["type"], "about:blank");
    assert_eq!(body["title"], status.canonical_reason().unwrap());
    assert_eq!(body["status"], status.as_u16());
    assert_eq!(body["instance"], instance);
    body
}

#[actix_web::test]
async fn health_returns_ok_json() {
    let (pool, token) = build_pool_and_token().await;
    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
    )
    .await;

    let req = test::TestRequest::get().uri("/api/health").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["status"], "ok");
}

#[actix_web::test]
async fn post_coords_requires_auth() {
    let (pool, token) = build_pool_and_token().await;

    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
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
    assert_eq!(
        resp.headers().get(header::WWW_AUTHENTICATE).unwrap(),
        "Bearer"
    );
    let body = read_problem(resp, StatusCode::UNAUTHORIZED, "/api/coords").await;
    assert_eq!(body["detail"], "Missing bearer token");

    // Wrong token -> 401
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer wrong"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.headers().get(header::WWW_AUTHENTICATE).unwrap(),
        r#"Bearer error="invalid_token""#
    );
    let body = read_problem(resp, StatusCode::UNAUTHORIZED, "/api/coords").await;
    assert_eq!(body["detail"], "Invalid bearer token");

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

    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
    )
    .await;

    // Not found initially
    let req = test::TestRequest::get().uri("/api/coords/bob").to_request();
    let resp = test::call_service(&app, req).await;
    let body = read_problem(resp, StatusCode::NOT_FOUND, "/api/coords/bob").await;
    assert_eq!(body["detail"], "No user named 'bob'");

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
async fn post_coords_with_motion_round_trips_and_clears() {
    let (pool, token) = build_pool_and_token().await;

    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
    )
    .await;

    // Speed and heading are stored and returned
    let payload = serde_json::json!({
        "name": "Carol",
        "latitude": 59.3,
        "longitude": 18.0,
        "speed": 8.3,
        "heading": 45.0
    });
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["speed"], 8.3);
    assert_eq!(body["heading"], 45.0);

    let req = test::TestRequest::get()
        .uri("/api/coords/carol")
        .to_request();
    let body: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(body["speed"], 8.3);
    assert_eq!(body["heading"], 45.0);

    // An update without motion data clears it; the fields are omitted from the JSON
    let payload = serde_json::json!({
        "name": "Carol",
        "latitude": 59.31,
        "longitude": 18.01
    });
    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = test::TestRequest::get().uri("/api/coords").to_request();
    let body: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    let carol = &body.as_array().expect("array")[0];
    assert_eq!(carol["latitude"], 59.31);
    assert!(carol.get("speed").is_none());
    assert!(carol.get("heading").is_none());
}

#[actix_web::test]
async fn post_coords_rejects_invalid_motion() {
    let (pool, token) = build_pool_and_token().await;

    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
    )
    .await;

    for (speed, heading) in [(-1.0, 90.0), (5.0, 400.0), (5.0, -10.0)] {
        let payload = serde_json::json!({
            "name": "dave",
            "latitude": 1.0,
            "longitude": 2.0,
            "speed": speed,
            "heading": heading
        });
        let req = test::TestRequest::post()
            .uri("/api/coords")
            .insert_header((header::AUTHORIZATION, "Bearer test-token"))
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;
        let body = read_problem(resp, StatusCode::BAD_REQUEST, "/api/coords").await;
        assert_eq!(body["errors"].as_array().unwrap().len(), 1, "{body}");
    }
}

#[actix_web::test]
async fn post_coords_reports_every_invalid_field() {
    let app = init_app().await;

    let req = test::TestRequest::post()
        .uri("/api/coords")
        .insert_header((header::AUTHORIZATION, "Bearer test-token"))
        .set_json(serde_json::json!({
            "name": "eve",
            "latitude": 91.0,
            "longitude": 2.0,
            "heading": 400.0
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let body = read_problem(resp, StatusCode::BAD_REQUEST, "/api/coords").await;
    assert_eq!(body["detail"], "2 fields are invalid");
    assert_eq!(
        body["errors"],
        serde_json::json!([
            { "pointer": "#/latitude", "detail": "must be between -90 and 90" },
            { "pointer": "#/heading", "detail": "must be between 0 and 360" }
        ])
    );
}

#[actix_web::test]
async fn unparseable_requests_get_problem_details() {
    let app = init_app().await;
    let post = || {
        test::TestRequest::post()
            .uri("/api/coords")
            .insert_header((header::AUTHORIZATION, "Bearer test-token"))
    };

    // Malformed JSON
    let req = post()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .set_payload("{not json")
        .to_request();
    let resp = test::call_service(&app, req).await;
    let body = read_problem(resp, StatusCode::BAD_REQUEST, "/api/coords").await;
    assert!(
        body["detail"]
            .as_str()
            .unwrap()
            .starts_with("Invalid JSON body")
    );

    // Client-sent timestamp is an unknown field
    let req = post()
        .set_json(serde_json::json!({
            "name": "a", "latitude": 1.0, "longitude": 2.0, "timestamp": "2020-01-01T00:00:00Z"
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let body = read_problem(resp, StatusCode::BAD_REQUEST, "/api/coords").await;
    assert!(
        body["detail"]
            .as_str()
            .unwrap()
            .contains("unknown field `timestamp`")
    );

    // Wrong content type
    let req = post()
        .insert_header((header::CONTENT_TYPE, "text/plain"))
        .set_payload(r#"{"name":"a","latitude":1.0,"longitude":2.0}"#)
        .to_request();
    let resp = test::call_service(&app, req).await;
    read_problem(resp, StatusCode::UNSUPPORTED_MEDIA_TYPE, "/api/coords").await;

    // Invalid query string on the websocket endpoint fails before the upgrade
    let req = test::TestRequest::get()
        .uri("/ws/coords?initial=yes")
        .to_request();
    let resp = test::call_service(&app, req).await;
    let body = read_problem(resp, StatusCode::BAD_REQUEST, "/ws/coords").await;
    assert!(body["detail"].as_str().unwrap().contains("yes"));

    // A plain GET without upgrade headers is a failed handshake
    let req = test::TestRequest::get().uri("/ws/coords").to_request();
    let resp = test::call_service(&app, req).await;
    read_problem(resp, StatusCode::BAD_REQUEST, "/ws/coords").await;
}

#[actix_web::test]
async fn unknown_routes_and_methods_get_problem_details() {
    let app = init_app().await;

    for path in ["/nope", "/api/nope", "/ws/nope"] {
        let req = test::TestRequest::get().uri(path).to_request();
        let resp = test::call_service(&app, req).await;
        let body = read_problem(resp, StatusCode::NOT_FOUND, path).await;
        assert_eq!(body["detail"], "No such endpoint", "{path}");
    }

    let req = test::TestRequest::put()
        .uri("/api/coords")
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = test::call_service(&app, req).await;
    // The middleware keeps the headers actix set, so clients still learn the allowed methods
    assert_eq!(resp.headers().get(header::ALLOW).unwrap(), "POST, GET");
    read_problem(resp, StatusCode::METHOD_NOT_ALLOWED, "/api/coords").await;
}

#[actix_web::test]
async fn options_does_not_500() {
    let (pool, token) = build_pool_and_token().await;

    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
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
    let server = HttpServer::new(move || {
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api)
    })
    .listen(listener)
    .expect("listen")
    .run();

    // Run server in background
    let _handle = actix_web::rt::spawn(server);

    // Call health endpoint using real HTTP client
    let client = Client::default();
    let url = format!("http://{addr}/api/health");
    let mut resp = client.get(url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[actix_web::test]
async fn delete_requires_auth() {
    let (pool, token) = build_pool_and_token().await;

    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
    )
    .await;

    // No auth -> 401
    let req = test::TestRequest::delete()
        .uri("/api/coords/any")
        .to_request();
    let resp = test::call_service(&app, req).await;
    read_problem(resp, StatusCode::UNAUTHORIZED, "/api/coords/any").await;

    // Wrong token -> 401
    let req = test::TestRequest::delete()
        .uri("/api/coords/any")
        .insert_header((header::AUTHORIZATION, "Bearer wrong"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    read_problem(resp, StatusCode::UNAUTHORIZED, "/api/coords/any").await;
}

#[actix_web::test]
async fn delete_flow_create_then_delete_then_404() {
    let (pool, token) = build_pool_and_token().await;

    let app = test::init_service(
        App::new()
            .app_data(make_state(pool.clone(), token.clone()))
            .configure(configure_api),
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
    let body = read_problem(resp, StatusCode::NOT_FOUND, "/api/coords/charlie").await;
    assert_eq!(body["detail"], "No user named 'charlie'");
}
