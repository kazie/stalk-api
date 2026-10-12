Live position updates over WebSocket. Connect with a plain `GET` and WebSocket upgrade
headers (e.g. `ws://host:8080/ws/coords`). No authentication is needed.

**Frames.** The server sends JSON text frames, each one a `WsEvent` with a `type` field:

```json
{ "type": "update", "name": "kazie", "latitude": 59.3272, "longitude": 18.1071, "speed": 8.3, "heading": 45.0, "timestamp": "2025-09-27T14:20:00.000Z" }
{ "type": "delete", "name": "kazie" }
```

- `update` is sent after every successful `POST /api/coords`. It carries the stored position
  with the same fields as `UserCoords`.
- `delete` is sent after a successful `DELETE /api/coords/{name}` and carries only the
  normalized name from the request.

**Snapshots.** `/ws/coords?initial=true` first sends one `update` per user active in the last
60 minutes (at most 20), then switches to live events. `/ws/coords/{name}` always starts with
that user's position if it was updated in the last 60 minutes.

**Errors.** A request that can't be upgraded, or has an invalid `initial` value, gets a `400`
Problem Details response instead of a WebSocket.

**Connection.** The server pings every 30 seconds. Text and binary frames from the client are
ignored, client pings get a pong, and a close frame ends the stream. The server buffers up to
1024 events. A client that falls further behind loses the oldest ones and carries on from there,
so re-fetch `GET /api/coords` if you need a full picture again.
