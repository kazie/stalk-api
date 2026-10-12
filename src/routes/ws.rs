use crate::models::WsEvent;
use crate::models::normalize_name;
use crate::problem::ProblemDetails;
use crate::routes::UserPath;
use crate::{
    AppState,
    db::{get_all_coords_time_limited, get_specific_user_coords_time_limited},
};
use actix_web::{Error as ActixError, HttpRequest, HttpResponse, get, web};
use actix_ws::Message;
use futures_util::StreamExt;
use log::{debug, error, info, warn};
use std::time::Duration;

#[derive(serde::Deserialize)]
struct WsAllUsersQuery {
    /// If true, server sends an initial load as individual UserCoords frames before live updates.
    /// Accepts only `true`/`false` or `1`/`0` (as strings are typical in query params). Other values are rejected.
    #[serde(default, deserialize_with = "de_bool_or_one")]
    initial: bool,
}

fn de_bool_or_one<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error as DeError, Unexpected};
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string 'true'/'false' or '1'/'0'")
        }
        fn visit_str<E: DeError>(self, v: &str) -> Result<bool, E> {
            let s = v.trim().to_ascii_lowercase();
            match s.as_str() {
                "true" | "1" => Ok(true),
                "false" | "0" => Ok(false),
                _ => Err(E::invalid_value(
                    Unexpected::Str(v),
                    &"'true'/'false' or '1'/'0'",
                )),
            }
        }
    }
    // Actix Query params are strings; deserialize from string only.
    deserializer.deserialize_str(Visitor)
}

/// Stream updates for all users
///
/// Upgrades to a WebSocket that receives a `WsEvent` JSON text frame for every position update
/// and deletion, for any user. With `?initial=true`, it first sends one `update` frame per user
/// active in the last 60 minutes (at most 20), so the client gets a full picture without a
/// separate `GET /api/coords`.
///
/// This cannot be tried from the docs page, so use a WebSocket client such as `websocat`.
#[utoipa::path(
    get,
    path = "/ws/coords",
    params(
        ("initial" = Option<bool>, Query, description = "Send a snapshot of recently active users before live events. Accepts `true`/`false` or `1`/`0`; defaults to `false`", example = true)
    ),
    responses(
        (status = 101, description = "Switching Protocols. The connection becomes a stream of `WsEvent` JSON text frames", body = WsEvent),
        (status = 400, description = "Not a valid WebSocket upgrade request, or `initial` is not `true`/`false`/`1`/`0`",
            body = ProblemDetails, content_type = "application/problem+json",
            example = json!({
                "type": "about:blank",
                "title": "Bad Request",
                "status": 400,
                "detail": "Query deserialize error: invalid value: string \"yes\", expected 'true'/'false' or '1'/'0'",
                "instance": "/ws/coords"
            }))
    ),
    tag = "websocket"
)]
#[get("/coords")]
pub async fn ws_coords(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
    query: web::Query<WsAllUsersQuery>,
) -> Result<HttpResponse, ActixError> {
    let (resp, mut ws_session, mut client_incoming) = match actix_ws::handle(&req, stream) {
        Ok(parts) => parts,
        Err(err) => {
            error!("WS handshake failed: {err:?}");
            return Err(err);
        }
    };

    let initial = query.initial;
    info!("WS connected: all-users initial={initial}");

    // If requested, send an initial load as individual update frames
    if initial {
        match get_all_coords_time_limited(&state.db).await {
            Ok(list) => {
                for coords in list {
                    let event = WsEvent::Update(coords);
                    match serde_json::to_string(&event) {
                        Ok(s) => {
                            if let Err(e) = ws_session.text(s).await {
                                debug!("WS send error during initial snapshot, closing: {e:?}");
                                let _ = ws_session.close(None).await;
                                return Ok(resp);
                            }
                        }
                        Err(e) => {
                            error!("Serialize error during initial snapshot: {e:?}");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Initial snapshot fetch failed: {e:?}");
            }
        }
    }

    let mut updates_receiver = state.notifier.subscribe();

    actix_web::rt::spawn(async move {
        let mut hb = actix_web::rt::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                // Heartbeat tick
                _ = hb.tick() => {
                    if let Err(e) = ws_session.ping(b"ping").await {
                        debug!("WS heartbeat send error, closing: {e:?}");
                        break;
                    }
                }
                // Receive broadcast of updates
                update_result = updates_receiver.recv() => {
                    match update_result {
                        Ok(event) => {
                            let json = match serde_json::to_string(&event) {
                                Ok(s) => s,
                                Err(e) => { error!("Serialize error: {e:?}"); break; }
                            };
                            if let Err(e) = ws_session.text(json).await {
                                debug!("WS send error, closing: {e:?}");
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!("WS consumer lagged by {n} messages; dropping to latest");
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            debug!("WS broadcaster closed");
                            break;
                        }
                    }
                }
                // Handle client messages (we don't expect any; just react to close)
                incoming = client_incoming.next() => {
                    match incoming {
                        Some(Ok(Message::Close(_))) => { debug!("WS client closed"); break; }
                        Some(Ok(Message::Ping(bytes))) => { let _ = ws_session.pong(&bytes).await; }
                        Some(Ok(Message::Text(_)| Message::Binary(_)| Message::Continuation(_))) => { /* ignore */ }
                        Some(Ok(Message::Pong(_))) => { /* ignore */ }
                        Some(Ok(Message::Nop)) => { /* ignore */ }
                        None => { break; }
                        Some(Err(e)) => { debug!("WS protocol error: {e:?}"); break; }
                    }
                }
            }
        }
        let _ = ws_session.close(None).await;
        info!("WS disconnected: all-users");
    });

    Ok(resp)
}

/// Stream updates for one user
///
/// Upgrades to a WebSocket that receives `WsEvent` JSON text frames for this user only: an
/// `update` for every new position and a `delete` if the user is removed. If the user was
/// updated in the last 60 minutes, that position is sent first as an `update` frame.
///
/// The user does not have to exist yet. The stream stays open after a `delete` and picks up
/// the user again if they post a new position.
///
/// This cannot be tried from the docs page, so use a WebSocket client such as `websocat`.
#[utoipa::path(
    get,
    path = "/ws/coords/{name}",
    params(UserPath),
    responses(
        (status = 101, description = "Switching Protocols. The connection becomes a stream of this user's `WsEvent` JSON text frames", body = WsEvent),
        (status = 400, description = "Not a valid WebSocket upgrade request",
            body = ProblemDetails, content_type = "application/problem+json")
    ),
    tag = "websocket"
)]
#[get("/coords/{name}")]
pub async fn ws_coords_user(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
    path: web::Path<UserPath>,
) -> Result<HttpResponse, ActixError> {
    let (resp, mut ws_session, mut client_incoming) = match actix_ws::handle(&req, stream) {
        Ok(parts) => parts,
        Err(err) => {
            error!("WS handshake failed: {err:?}");
            return Err(err);
        }
    };

    let username = normalize_name(&path.name);
    info!("WS connected: user={username}");

    // Optionally send a snapshot if available
    if let Ok(Some(snapshot)) = get_specific_user_coords_time_limited(&state.db, &username).await
        && let Ok(json) = serde_json::to_string(&WsEvent::Update(snapshot))
    {
        let _ = ws_session.text(json).await;
    }

    let mut updates_receiver = state.notifier.subscribe();

    actix_web::rt::spawn(async move {
        let mut hb = actix_web::rt::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                // Heartbeat tick
                _ = hb.tick() => {
                    if let Err(e) = ws_session.ping(b"ping").await {
                        debug!("WS heartbeat send error, closing: {e:?}");
                        break;
                    }
                }
                update_result = updates_receiver.recv() => {
                    match update_result {
                        Ok(event) => {
                            // names are stored normalized in DB, compare case-insensitively
                            let matches = match &event {
                                WsEvent::Update(coords) => coords.name.eq_ignore_ascii_case(&username),
                                WsEvent::Delete { name } => name.eq_ignore_ascii_case(&username),
                            };
                            if matches {
                                let json = match serde_json::to_string(&event) { Ok(s) => s, Err(e) => { error!("Serialize error: {e:?}"); break; } };
                                if let Err(e) = ws_session.text(json).await { debug!("WS send error, closing: {e:?}"); break; }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => { warn!("WS consumer lagged by {n} messages; dropping to latest"); continue; }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => { debug!("WS broadcaster closed"); break; }
                    }
                }
                incoming = client_incoming.next() => {
                    match incoming {
                        Some(Ok(Message::Close(_))) => { debug!("WS client closed"); break; }
                        Some(Ok(Message::Ping(bytes))) => { let _ = ws_session.pong(&bytes).await; }
                        Some(Ok(Message::Text(_)| Message::Binary(_)| Message::Continuation(_))) => { /* ignore */ }
                        Some(Ok(Message::Pong(_))) => { /* ignore */ }
                        Some(Ok(Message::Nop)) => { /* ignore */ }
                        None => { break; }
                        Some(Err(e)) => { debug!("WS protocol error: {e:?}"); break; }
                    }
                }
            }
        }
        let _ = ws_session.close(None).await;
        info!("WS disconnected: user={username}");
    });

    Ok(resp)
}
