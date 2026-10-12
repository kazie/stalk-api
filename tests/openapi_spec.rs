use actix_web::{App, test, web};
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::problem::PROBLEM_JSON;
use stalk_api::{AppState, configure_api};

#[actix_web::test]
async fn openapi_json_and_uis_are_served_under_api_scope() {
    // Prepare a minimal state; the doc/health endpoints do not hit the DB, but we provide a pool.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");

    let (coords_update_sender, _unused_receiver) = tokio::sync::broadcast::channel(16);
    let state = AppState {
        db: pool,
        auth_token: "test-token".to_string(),
        notifier: coords_update_sender,
    };

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
    assert!(
        resp.status().is_success(),
        "Expected 200, got {:?}",
        resp.status()
    );

    let body = test::read_body(resp).await;
    let body_str = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        body_str.contains("/api/coords"),
        "OpenAPI JSON should contain /api/coords path",
    );

    // Verify timestamp is documented as RFC 3339 date-time formatted string
    let json: serde_json::Value = serde_json::from_str(&body_str).expect("valid json");
    let ts_format =
        json["components"]["schemas"]["UserCoords"]["properties"]["timestamp"]["format"].as_str();
    assert_eq!(
        ts_format,
        Some("date-time"),
        "timestamp should have format=date-time in OpenAPI schema"
    );

    // Verify the optional motion fields are documented on both input and output
    for schema in ["UserCoords", "NewUserCoords"] {
        let props = &json["components"]["schemas"][schema]["properties"];
        for field in ["speed", "heading"] {
            assert!(
                props[field].is_object(),
                "{schema}.{field} should be documented in OpenAPI schema"
            );
        }
        let required = json["components"]["schemas"][schema]["required"]
            .as_array()
            .expect("required list");
        assert!(
            !required.iter().any(|r| r == "speed" || r == "heading"),
            "{schema} speed/heading should be optional"
        );
    }

    // Stored headings are normalized, so responses never contain 360
    let heading = &json["components"]["schemas"]["UserCoords"]["properties"]["heading"];
    assert_eq!(heading["exclusiveMaximum"], 360.0);

    // Verify HealthResponse has example for status
    let health_status_example =
        json["components"]["schemas"]["HealthResponse"]["properties"]["status"]["example"].as_str();
    assert_eq!(
        health_status_example,
        Some("ok"),
        "HealthResponse.status should have example 'ok'"
    );

    // Assert RapiDoc UI is served under /api
    let rapidoc_req = test::TestRequest::with_uri("/api/rapidoc").to_request();
    let rapidoc_resp = test::call_service(&app, rapidoc_req).await;
    assert!(
        rapidoc_resp.status().is_success(),
        "RapiDoc UI should return success"
    );

    // Assert Swagger UI is served under /api
    let swagger_req = test::TestRequest::with_uri("/api/swagger-ui/").to_request();
    let swagger_resp = test::call_service(&app, swagger_req).await;
    assert!(
        swagger_resp.status().is_success(),
        "Swagger UI should return success"
    );
}

#[actix_web::test]
async fn openapi_spec_is_documented_for_rapidoc() {
    use utoipa::OpenApi;
    let json = serde_json::to_value(stalk_api::openapi::ApiDoc::openapi()).unwrap();

    // The intro page RapiDoc renders comes from src/openapi.md
    let intro = json["info"]["description"]
        .as_str()
        .expect("info.description");
    for section in [
        "## Authentication",
        "## User names",
        "## Freshness",
        "## Errors",
    ] {
        assert!(
            intro.contains(section),
            "intro should have a {section} section"
        );
    }
    assert!(
        intro.contains("last 60\nminutes"),
        "intro should explain the 60-minute window"
    );

    // Every operation shows up in RapiDoc with a summary and a description
    for (path, item) in json["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            for field in ["summary", "description"] {
                assert!(
                    op[field].as_str().is_some_and(|s| !s.trim().is_empty()),
                    "{method} {path} should have a {field}"
                );
            }
        }
    }

    // WebSocket endpoints get their own documented section
    for path in ["/ws/coords", "/ws/coords/{name}"] {
        assert_eq!(
            json["paths"][path]["get"]["tags"],
            serde_json::json!(["websocket"])
        );
    }
    let tags = json["tags"].as_array().unwrap();
    for tag in tags {
        assert!(
            tag["description"].as_str().is_some_and(|s| !s.is_empty()),
            "tag {} should have a description",
            tag["name"]
        );
    }
    assert!(tags.iter().any(|t| t["name"] == "websocket"));

    // `initial` is optional, it defaults to false
    let initial = &json["paths"]["/ws/coords"]["get"]["parameters"][0];
    assert_eq!(initial["name"], "initial");
    assert_eq!(initial["required"], false);

    // Authenticated endpoints document the 401
    assert!(json["paths"]["/api/coords"]["post"]["responses"]["401"].is_object());
    assert!(json["paths"]["/api/coords/{name}"]["delete"]["responses"]["401"].is_object());
    assert!(
        json["components"]["securitySchemes"]["ApiToken"]["description"]
            .as_str()
            .is_some_and(|s| s.contains("API_TOKEN"))
    );

    // Errors are documented as RFC 9457 problem details
    let problem = &json["components"]["schemas"]["ProblemDetails"]["properties"];
    for member in ["type", "title", "status", "detail", "instance", "errors"] {
        assert!(
            problem[member].is_object(),
            "ProblemDetails.{member} should be documented"
        );
    }
    let post_400 = &json["paths"]["/api/coords"]["post"]["responses"]["400"]["content"];
    assert_eq!(
        post_400[PROBLEM_JSON]["example"]["errors"][0]["pointer"],
        "#/latitude"
    );
    for (path, item) in json["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            for (status, response) in op["responses"].as_object().unwrap() {
                // Shared responses are references into components.responses
                let response = match response["$ref"].as_str() {
                    Some(r) => {
                        &json["components"]["responses"]
                            [r.trim_start_matches("#/components/responses/")]
                    }
                    None => response,
                };
                if status.starts_with('4') || status.starts_with('5') {
                    assert!(
                        response["content"][PROBLEM_JSON].is_object(),
                        "{method} {path} {status} should be application/problem+json"
                    );
                }
            }
        }
    }

    // Input schema documents its constraints, without the doctests from its doc comment
    let new_coords = &json["components"]["schemas"]["NewUserCoords"];
    assert_eq!(new_coords["additionalProperties"], false);
    assert!(!new_coords["description"].as_str().unwrap().contains("```"));
    for schema in ["UserCoords", "NewUserCoords"] {
        let props = &json["components"]["schemas"][schema]["properties"];
        assert_eq!(props["latitude"]["minimum"], -90.0);
        assert_eq!(props["latitude"]["maximum"], 90.0);
        assert_eq!(props["longitude"]["minimum"], -180.0);
        assert_eq!(props["longitude"]["maximum"], 180.0);
        for field in ["name", "latitude", "longitude"] {
            assert!(
                props[field]["description"].is_string(),
                "{schema}.{field} should have a description"
            );
        }
    }
}
