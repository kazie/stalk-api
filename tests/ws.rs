use actix_web::{App, HttpServer, web};
use futures_util::StreamExt;
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use stalk_api::utils::ws_url;
use stalk_api::{AppState, configure_api, models::NewUserCoords};
use std::net::TcpListener;

async fn setup_pool() -> sqlx::Pool<sqlx::Sqlite> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    pool
}

#[actix_web::test]
async fn ws_all_users_stream_receives_updates() {
    let pool = setup_pool().await;
    let token = "test-token".to_string();
    let (coords_update_sender, _unused_receiver) = tokio::sync::broadcast::channel(64);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token.clone(),
                notifier: coords_update_sender.clone(),
            }))
            .configure(configure_api)
    })
    .listen(listener)
    .expect("listen")
    .run();
    let _handle = actix_web::rt::spawn(server);
    let base_url = format!("http://{}", addr);

    let client = awc::Client::new();
    // Connect WebSocket
    let url = ws_url(&base_url, "/ws/coords");
    let (_resp, mut ws) = client.ws(url).connect().await.expect("ws connect");

    // Trigger an update via REST
    let body = NewUserCoords {
        name: "Alice".into(),
        latitude: 10.0,
        longitude: 20.0,
    };
    let resp = client
        .post(format!("{}/api/coords", base_url))
        .insert_header(("Authorization", format!("Bearer {}", "test-token")))
        .send_json(&body)
        .await
        .expect("post ok");
    assert!(resp.status().is_success());

    // Expect to receive a WS message
    let frame = ws.next().await.expect("one message").expect("ok frame");
    match frame {
        awc::ws::Frame::Text(bytes) => {
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            let json: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(json["name"], "Alice");
            assert_eq!(json["latitude"], 10.0);
            assert_eq!(json["longitude"], 20.0);
        }
        other => panic!("unexpected frame: {:?}", other),
    }
}

#[actix_web::test]
async fn ws_per_user_isolation_and_case_insensitive() {
    let pool = setup_pool().await;
    let token = "test-token".to_string();
    let (coords_update_sender, _unused_receiver) = tokio::sync::broadcast::channel(64);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token.clone(),
                notifier: coords_update_sender.clone(),
            }))
            .configure(configure_api)
    })
    .listen(listener)
    .expect("listen")
    .run();
    let _handle = actix_web::rt::spawn(server);
    let base_url = format!("http://{}", addr);

    let client = awc::Client::new();
    let url = ws_url(&base_url, "/ws/coords/ALICE");
    let (_resp, mut ws) = client.ws(url).connect().await.expect("ws connect");

    // Send updates for Alice and Bob, mixed case
    for (name, lat) in [("Alice", 1.0), ("BOB", 2.0), ("aLiCe", 3.0)] {
        let body = NewUserCoords {
            name: name.into(),
            latitude: lat,
            longitude: 0.0,
        };
        let resp = client
            .post(format!("{}/api/coords", base_url))
            .insert_header(("Authorization", format!("Bearer {}", "test-token")))
            .send_json(&body)
            .await
            .expect("post ok");
        assert!(resp.status().is_success());
    }

    // We expect to receive only Alice updates (2 messages)
    let mut received = vec![];
    for _ in 0..2 {
        if let Some(Ok(awc::ws::Frame::Text(bytes))) = ws.next().await {
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            let json: Value = serde_json::from_str(&text).unwrap();
            received.push(json);
        }
    }
    assert_eq!(received.len(), 2);
    assert!(received.iter().all(|j| j["name"] == "Alice"));
}

#[actix_web::test]
async fn ws_per_user_snapshot_on_connect() {
    let pool = setup_pool().await;
    let token = "test-token".to_string();
    let (coords_update_sender, _unused_receiver) = tokio::sync::broadcast::channel(64);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token.clone(),
                notifier: coords_update_sender.clone(),
            }))
            .configure(configure_api)
    })
    .listen(listener)
    .expect("listen")
    .run();
    let _handle = actix_web::rt::spawn(server);
    let base_url = format!("http://{}", addr);

    let client = awc::Client::new();

    // Pre-insert user via REST
    let body = NewUserCoords {
        name: "Snap".into(),
        latitude: 42.0,
        longitude: 24.0,
    };
    let resp = client
        .post(format!("{}/api/coords", base_url))
        .insert_header(("Authorization", format!("Bearer {}", "test-token")))
        .send_json(&body)
        .await
        .expect("post ok");
    assert!(resp.status().is_success());

    // Connect to per-user WS and expect an immediate snapshot message
    let url = ws_url(&base_url, "/ws/coords/SNAP");
    let (_resp, mut ws) = client.ws(url).connect().await.expect("ws connect");

    if let Some(Ok(awc::ws::Frame::Text(bytes))) = ws.next().await {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let json: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["name"], "Snap");
        assert_eq!(json["latitude"], 42.0);
        assert_eq!(json["longitude"], 24.0);
    } else {
        panic!("expected snapshot frame");
    }
}

#[actix_web::test]
async fn ws_all_users_initial_snapshot_like_updates() {
    let pool = setup_pool().await;
    let token = "test-token".to_string();
    let (coords_update_sender, _unused_receiver) = tokio::sync::broadcast::channel(64);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState {
                db: pool.clone(),
                auth_token: token.clone(),
                notifier: coords_update_sender.clone(),
            }))
            .configure(configure_api)
    })
    .listen(listener)
    .expect("listen")
    .run();
    let _handle = actix_web::rt::spawn(server);
    let base_url = format!("http://{}", addr);

    let client = awc::Client::new();

    // Pre-insert two users via REST
    for (name, lat) in [("Alice", 1.0_f64), ("Bob", 2.0_f64)] {
        let body = NewUserCoords {
            name: name.into(),
            latitude: lat,
            longitude: 0.0,
        };
        let resp = client
            .post(format!("{}/api/coords", base_url))
            .insert_header(("Authorization", format!("Bearer {}", "test-token")))
            .send_json(&body)
            .await
            .expect("post ok");
        assert!(resp.status().is_success());
    }

    // Connect with initial=1; expect to receive two frames representing Alice and Bob, order not guaranteed
    let url = ws_url(&base_url, "/ws/coords?initial=1");
    let (_resp, mut ws) = client.ws(url).connect().await.expect("ws connect");

    let mut names = vec![];
    for _ in 0..2 {
        if let Some(Ok(awc::ws::Frame::Text(bytes))) = ws.next().await {
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            let json: Value = serde_json::from_str(&text).unwrap();
            names.push(json["name"].as_str().unwrap().to_string());
        } else {
            panic!("expected text frame");
        }
    }
    names.sort();
    assert_eq!(names, vec!["Alice".to_string(), "Bob".to_string()]);

    // Now send a new update and check it arrives after the initial ones
    let body = NewUserCoords {
        name: "Alice".into(),
        latitude: 9.9,
        longitude: 9.9,
    };
    let resp = client
        .post(format!("{}/api/coords", base_url))
        .insert_header(("Authorization", format!("Bearer {}", "test-token")))
        .send_json(&body)
        .await
        .expect("post ok");
    assert!(resp.status().is_success());

    if let Some(Ok(awc::ws::Frame::Text(bytes))) = ws.next().await {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let json: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["name"], "Alice");
        assert_eq!(json["latitude"], 9.9);
        assert_eq!(json["longitude"], 9.9);
    } else {
        panic!("expected update frame after initial snapshot");
    }
}
