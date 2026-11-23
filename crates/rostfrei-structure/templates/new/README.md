# {{context_label}}

This project uses `rostfrei = "={{rostfrei_version}}"` from crates.io with the `nats`
feature. Install rustup and Docker with Compose. Commit `Cargo.lock` after the
first `cargo check`. To check the starter without a broker:

```sh
cargo check
cargo test --locked
cargo rostfrei check
```

## Commit and replay a rental

```sh
docker compose up -d
cargo run --locked --example first_rental
```

On a new local store this prints `rental-1: committed; retry: replayed; stored events: 1`.
Run it again against the same broker: even the first command now replays. The example
checks both receipts against the bicycle's stored history, uses the project's Test
namespace, and never resets it. `ROSTFREI_NATS_URL` defaults to `nats://127.0.0.1:4222`;
use an isolated development broker if you override it. `docker compose down` stops
the broker and keeps the volume; `docker compose down -v` deletes the local data.

Your own domain starts in `src/domain/`, independent of this removable example.
`cargo run` starts the connection-only application and waits for Ctrl-C.
If you generated with `--no-skills`, `cargo rostfrei skills install` adds the
release-matched Agent Skills after `cargo check` has created the lockfile.
