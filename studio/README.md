# Rostfrei Studio

Desktop IDE for browsing and validating compiled Rostfrei domain models.

## Development

```bash
npm install
npm run tauri -- dev
```

## Verification

```bash
npm run build
npm test
npm run lint
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

The Tauri backend is an independent Rust workspace under `src-tauri/`. The UI
loads versioned domain JSON through native commands, builds a canonical domain
index, and runs Cargo checks against the selected workspace.
