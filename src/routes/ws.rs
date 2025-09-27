use crate::models::normalize_name;
use crate::{
    AppState,
    db::{get_all_coords_time_limited, get_specific_user_coords_time_limited},
};
use actix_web::{Error as ActixError, HttpRequest, HttpResponse, get, web};
use actix_ws::Message;
use futures_util::StreamExt;
use log::{debug, error, info, warn};

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

/// WebSocket endpoint streaming all user coordinate updates.
#[utoipa::path(
    get,
    path = "/ws/coords",
    params(
        ("initial" = bool, Query, description = "When true (accepts only true/false or 1/0), send an initial load of all current users as individual update frames before streaming live updates")
    ),
    responses(
        (status = 101, description = "Switching Protocols: WebSocket upgrade to a stream of JSON UserCoords messages"),
        (status = 400, description = "Bad request")
    ),
    tag = "coords"
)]
#[get("/ws/coords")]
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
                    match serde_json::to_string(&coords) {
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
        loop {
            tokio::select! {
                // Receive broadcast of updates
                update_result = updates_receiver.recv() => {
                    match update_result {
                        Ok(coords) => {
                            let json = match serde_json::to_string(&coords) {
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

/// WebSocket endpoint streaming updates for a single user. Optionally sends snapshot on connect.
#[utoipa::path(
    get,
    path = "/ws/coords/{name}",
    params(
        ("name" = String, Path, description = "User name (case-insensitive)")
    ),
    responses(
        (status = 101, description = "Switching Protocols: WebSocket upgrade to a stream of JSON UserCoords messages filtered by user"),
        (status = 400, description = "Bad request")
    ),
    tag = "coords"
)]
#[get("/ws/coords/{name}")]
pub async fn ws_coords_user(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
    name: web::Path<String>,
) -> Result<HttpResponse, ActixError> {
    let (resp, mut ws_session, mut client_incoming) = match actix_ws::handle(&req, stream) {
        Ok(parts) => parts,
        Err(err) => {
            error!("WS handshake failed: {err:?}");
            return Err(err);
        }
    };

    let name_raw = name.into_inner();
    let username = normalize_name(&name_raw);
    info!("WS connected: user={username}");

    // Optionally send a snapshot if available
    if let Ok(Some(snapshot)) = get_specific_user_coords_time_limited(&state.db, &username).await
        && let Ok(json) = serde_json::to_string(&snapshot)
    {
        let _ = ws_session.text(json).await;
    }

    let mut updates_receiver = state.notifier.subscribe();

    actix_web::rt::spawn(async move {
        loop {
            tokio::select! {
                update_result = updates_receiver.recv() => {
                    match update_result {
                        Ok(coords) => {
                            if coords.name.eq_ignore_ascii_case(&username) { // names are stored normalized in DB, compare case-insensitively
                                let json = match serde_json::to_string(&coords) { Ok(s) => s, Err(e) => { error!("Serialize error: {e:?}"); break; } };
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
