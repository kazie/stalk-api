Share and follow live positions. Clients post their latest location under a user name, and
anyone can read the current positions over REST or subscribe to changes over WebSocket.

## Authentication

Writes (`POST /api/coords`, `DELETE /api/coords/{name}`) need a bearer token:

```
Authorization: Bearer <API_TOKEN>
```

`API_TOKEN` is set on the server with the environment variable of the same name. A missing or
wrong token gets `401 Unauthorized`. Reads, the WebSocket streams and these docs are public.

## User names

A user is identified by name, and each name holds one position, the latest one.

- Names are trimmed and Unicode NFC-normalized before they are stored or looked up, so
  `" Café "` and `"Café"` are the same user.
- Lookups are case-insensitive for ASCII letters (`Kazie` = `kazie`). A name keeps the casing it
  was first stored with.
- A name has to be non-empty after trimming.

## Units and ranges

| Field       | Unit                                         | Range                        |
|-------------|----------------------------------------------|------------------------------|
| `latitude`  | WGS84 decimal degrees                        | `-90` to `90`                |
| `longitude` | WGS84 decimal degrees                        | `-180` to `180`              |
| `speed`     | metres per second, over ground               | `>= 0`, optional             |
| `heading`   | degrees clockwise from true north            | `0` to `360` (360 is stored as `0`), optional |
| `timestamp` | RFC 3339 UTC with milliseconds, set by the server | read-only                |

Each update replaces the whole position, so an update that leaves out `speed` or `heading` clears
them. Responses leave unknown `speed`/`heading` out instead of sending `null`.

## Freshness

`GET /api/coords` and the WebSocket snapshots only include positions updated in the **last 60
minutes**, and at most **20** users, in no particular order. `GET /api/coords/{name}` returns the
user's latest position however old it is.

## Errors

Every error, from any endpoint, is an [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) Problem
Details object with `Content-Type: application/problem+json`:

```json
{
  "type": "about:blank",
  "title": "Bad Request",
  "status": 400,
  "detail": "2 fields are invalid",
  "instance": "/api/coords",
  "errors": [
    { "pointer": "#/latitude", "detail": "must be between -90 and 90" },
    { "pointer": "#/heading", "detail": "must be between 0 and 360" }
  ]
}
```

| Member     | Meaning                                                                       |
|------------|-------------------------------------------------------------------------------|
| `type`     | Always `about:blank`: the HTTP status says what kind of problem it is         |
| `title`    | The HTTP reason phrase, e.g. `Not Found`                                      |
| `status`   | The HTTP status code                                                          |
| `detail`   | What went wrong in this case, for people. Don't parse it                      |
| `instance` | Path of the request that failed                                               |
| `errors`   | Validation errors only: every invalid field, with a JSON Pointer to it        |

| Status | When                                                                          |
|--------|-------------------------------------------------------------------------------|
| `400`  | Invalid field values (all listed in `errors`), malformed JSON, missing or unknown fields, bad query string, or a failed WebSocket handshake |
| `401`  | Bearer token missing or wrong. `WWW-Authenticate` is `Bearer`, or `Bearer error="invalid_token"` for a wrong token |
| `404`  | No such user, or no such endpoint                                             |
| `405`  | Method not supported on this path. `Allow` lists the ones that are            |
| `413`  | Request body too large                                                        |
| `415`  | Request body sent without `Content-Type: application/json`                    |
| `500`  | Server error. The cause is logged on the server and never sent to the client  |

## Live updates

Every successful `POST` and `DELETE` is pushed to WebSocket subscribers. See the **websocket**
section for the stream format.
