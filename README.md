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

If you ever delete `coords.sqlite` (e.g. while testing the server manually against a scratch
DB), regenerate it with one of the commands above before running `cargo build`/`test`/`clippy`
again — don't just delete it as throwaway state.

OpenAPI documentation
- Swagger UI: http://127.0.0.1:8080/api/swagger-ui
- RapiDoc UI: http://127.0.0.1:8080/api/rapidoc
- OpenAPI JSON: http://127.0.0.1:8080/api/openapi.json

The API documentation is generated from code using utoipa and stays in sync with the handlers and models.
