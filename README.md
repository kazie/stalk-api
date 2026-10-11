# stalk-api

The small core of the program, to provide API to put and get stalk data from.

## Development setup

`DATABASE_URL` (see `.env`) points at a local SQLite file, `coords.sqlite`. This file is
gitignored (`*.sqlite`) — it's never committed, so every clone/checkout needs to create its own.

**This file must exist and be migrated before `cargo build`, `cargo test`, or `cargo clippy`
will work.** `sqlx`'s compile-time query macros (`sqlx::query_as!` etc., used throughout
`src/db/`) connect to `DATABASE_URL` at *compile* time to type-check queries against the real
schema — without it, compilation fails with errors like `unable to open database file`, not just
runtime test failures.

Create/migrate it with the sqlx CLI (same as CI, see `.github/workflows/rust.yml`):

```sh
cargo install sqlx-cli --no-default-features --features sqlite # once
cargo sqlx database create
cargo sqlx migrate run
```

or equivalently, via the binary's own `--migrate` flag: `cargo run -- --migrate`.

When a new file lands in `migrations/`, run `cargo sqlx migrate run` again so the compile-time
checks see the new schema, and refresh the committed offline query cache with
`cargo sqlx prepare -- --all-targets`.

## Database migrations at runtime

On every startup the server applies any pending migrations to the database it opens, so deploying
a new version upgrades an existing database in place. Migrations already applied (tracked by sqlx
in `_sqlx_migrations`) are skipped. `--migrate` is still the way to create a brand-new database
file.

If you ever delete `coords.sqlite` (e.g. while testing the server manually against a scratch
DB), regenerate it with one of the commands above before running `cargo build`/`test`/`clippy`
again — don't just delete it as throwaway state.

OpenAPI documentation
- Swagger UI: http://127.0.0.1:8080/api/swagger-ui
- RapiDoc UI: http://127.0.0.1:8080/api/rapidoc
- OpenAPI JSON: http://127.0.0.1:8080/api/openapi.json

The API documentation is generated from code using utoipa and stays in sync with the handlers and models.

## Location payload

`POST /api/coords` takes:

```json
{ "name": "kazie", "latitude": 59.3272, "longitude": 18.1071, "speed": 8.3, "heading": 45.0 }
```

- `speed` (optional): speed over ground in **m/s**, must be ≥ 0.
- `heading` (optional): direction of travel in degrees clockwise from true north, `0`–`360`
  (`360` is stored as `0`).

Either one may be omitted or `null`. Each update replaces the previous values, so an update
without them clears them. Responses and websocket `update` events leave the keys out when they're
unknown. Any other unknown field is rejected with `400`.
