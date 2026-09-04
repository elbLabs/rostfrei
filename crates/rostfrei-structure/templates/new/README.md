# {{context_label}}

A Rostfrei application backed by NATS and JetStream.

## Run locally

Start NATS:

```console
docker compose up -d
```

Run the application:

```console
cargo run
```

Set `ROSTFREI_NATS_URL` to connect to a different server:

```console
ROSTFREI_NATS_URL=nats://example:4222 cargo run
```

## Quality checks

```console
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo rostfrei check
```

Stop NATS with `docker compose down`. Add `-v` to also delete the local JetStream data.
