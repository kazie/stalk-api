use actix_web::{test, App, web};
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::{configure_api, AppState};

#[actix_web::test]
async fn openapi_json_and_uis_are_served_under_api_scope() {
    // Prepare a minimal state; the doc/health endpoints do not hit the DB, but we provide a pool.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");

    let state = AppState { db: pool, auth_token: "test-token".to_string() };

    // Build app using the same configuration as production
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_api),
    )
    .await;

    // Assert OpenAPI JSON is served at /api/openapi.json
    let req = test::TestRequest::with_uri("/api/openapi.json").to_request();
    let resp = test::call_service(&app, req).await;
    assert!(resp.status().is_success(), "Expected 200, got {:?}", resp.status());

    let body = test::read_body(resp).await;
    let body_str = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        body_str.contains("/api/coords"),
        "OpenAPI JSON should contain /api/coords path",
    );

    // Verify timestamp is documented as RFC 3339 date-time formatted string
    let json: serde_json::Value = serde_json::from_str(&body_str).expect("valid json");
    let ts_format = json["components"]["schemas"]["UserCoords"]["properties"]["timestamp"]["format"]
        .as_str();
    assert_eq!(
        ts_format,
        Some("date-time"),
        "timestamp should have format=date-time in OpenAPI schema"
    );

    // Verify HealthResponse has example for status
    let health_status_example = json["components"]["schemas"]["HealthResponse"]["properties"]["status"]["example"]
        .as_str();
    assert_eq!(
        health_status_example,
        Some("ok"),
        "HealthResponse.status should have example 'ok'"
    );

    // Assert RapiDoc UI is served under /api
    let rapidoc_req = test::TestRequest::with_uri("/api/rapidoc").to_request();
    let rapidoc_resp = test::call_service(&app, rapidoc_req).await;
    assert!(rapidoc_resp.status().is_success(), "RapiDoc UI should return success");

    // Assert Swagger UI is served under /api
    let swagger_req = test::TestRequest::with_uri("/api/swagger-ui/").to_request();
    let swagger_resp = test::call_service(&app, swagger_req).await;
    assert!(swagger_resp.status().is_success(), "Swagger UI should return success");
}
