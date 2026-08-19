# {{context_label}}

A Rostfrei application backed by NATS and JetStream.

## Build and test

Install [rustup](https://rustup.rs/) and Git. The included `rust-toolchain.toml`
selects the Rust toolchain. No NATS server is needed for these checks:

```console
cargo check
cargo test
```

The manifest pins both Rostfrei dependencies to Git release `{{rostfrei_release}}`
at commit `{{rostfrei_revision}}`, because the crates are not yet published to
crates.io. The first build needs access to GitHub and crates.io. Commit the
generated `Cargo.lock` to retain the resolved transitive dependency versions.

The starter dependency pin is tested independently of the generator's version;
it can intentionally lag the CLI and `main`. To upgrade, select an available
compatible release commit for **both** dependencies, then run `cargo check` and
`cargo test` again.

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
cargo run --bin rostfrei-domain-check
```

If `cargo-rostfrei` is installed, `cargo rostfrei check` also checks the domain's
filesystem structure and runs the compiled domain-model check.

Stop NATS with `docker compose down`. Add `-v` to also delete the local JetStream data.
