# Agent Skills setup and use

Rostfrei provides two project-local skills for coding agents:

- **`rostfrei-develop-feature`** turns agreed Given/When/Then behavior into real
  NATS-backed tests, then guides the red/green/refactor loop.
- **`rostfrei-tracer`** discovers runtime capabilities, runs isolated tests and
  explains recorded operations, observations and quarantine through HTTP/SSE.

## Install and update

Rostfrei is a Cargo library dependency. Install its setup CLI separately; adding
the library, building it or starting an application does not install skills.
Install the 0.1.3 CLI from crates.io with Rust 1.98+, Cargo and a native linker.
Git dependencies also need Git and source access. Consumer setup and execution
need no Python.

```sh
cargo install --locked --version 0.1.3 rostfrei-structure
# Inside your application, with its dependency resolved in Cargo.lock:
cargo rostfrei skills install
# After deliberately upgrading the application's dependency and Cargo.lock:
cargo rostfrei skills update
```

The equivalent Git installation uses
`cargo install --locked --git https://github.com/elbLabs/rostfrei --tag v0.1.3 rostfrei-structure`.
Use the CLI/kit matching the application's actual locked framework revision;
the earlier 0.1.0 Git release and its kit retain their original identities.

Setup uses `cargo metadata --locked` to select the application's exact resolved
Rostfrei version and source revision, acquiring and caching its matching native
tool when needed. Resolve a missing/stale lockfile through Cargo first. Ambiguous
dependencies or unsupported revisions stop setup rather than selecting newer skills.
For a workspace member, use `--manifest-path /path/to/Cargo.toml --package my-app`.

Skills install beside the selected application's manifest in `.agents/skills`,
discoverable by OpenCode and Codex. Launch the agent in that application. Commit
both skill directories and `.agents/skills/.rostfrei-managed.json`, the managed
baseline recording their provenance and file hashes. Each installed skill's
`references/release.json` identifies its build; `skills info` identifies the invoked CLI.

Install is repeatable but does not upgrade an existing managed build; use update.
Update follows the application's dependency, preserves user-added/unrelated files,
and stops before changing either skill if managed files have local-edit conflicts.
Resolve the reported paths deliberately and rerun. Older unmanaged copies are
adopted only when complete and byte-identical; otherwise install elsewhere and merge.

**Published starter limitation:** `cargo rostfrei new` pins `v0.0.5-alpha`, which
predates skills support. It creates the application and reports that skills were
not installed. Setup cannot supply compatible skills for that dependency; a newer
setup CLI does not change the starter pin or make a skills-enabled release available.

## Run an application behavior test

Start the application's own Tracer host with its real handlers, workers, fixtures
and NATS event store on **NATS 2.12.1+ with JetStream**. For the framework example,
follow [bike-rental startup](https://github.com/elbLabs/rostfrei/blob/main/examples/bike-rental/README.md#run-locally):
export `ROSTFREI_NATS_URL`, a unique owned `ROSTFREI_APPLICATION`, a new explicit
`ROSTFREI_COMPLETED_OUTCOME_EPOCH`, and distinct `ROSTFREI_API_TOKEN` /
`ROSTFREI_DISPATCH_TOKEN`. Run `cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer`.
Without the flag there is no Tracer HTTP. The epoch selects fresh writable authority;
omitting it retains legacy/read-only semantics. Do not apply this fresh setup to an
existing history. A consuming application supplies its own application-linked host.

Use a disposable broker or uniquely owned application namespace and serialize Test
runs. Every behavior run resets the **entire configured Test scenario**, including
paired journal/outcome authority, messaging streams, quarantine, consumers and retained
Test evidence, reconnects bound services, then applies its fixture. Normal resources
must survive these resets. External stores
and read models reset only if the application implements that support; a run ID
alone provides no isolation. Stop the owned host/broker after use.

Have the Tracer skill discover the live `/catalog` and follow its advertised schema,
fixture and execution links. Use an application-owned JSON v2 definition with a
registered fixture; optional inspection/diagnostic capabilities depend on that host.
In the agent terminal, export the host's control `ROSTFREI_API_TOKEN`, then run:

```sh
export ROSTFREI_TRACER_URL=http://127.0.0.1:1309
cargo rostfrei test tests/my-behavior.json --output my-evidence.json \
  --release .agents/skills/rostfrei-develop-feature/references/release.json
```

Exit **0** means a reported pass with complete evidence retrieval; **1** means a
reported failed suite with complete retrieval; **2** means blocked/incomplete
execution. A later capture failure retains the received report/verdict but exits 2.
Existing evidence is never overwritten: choose a new output path for each run.
Never automatically retry an uncertain submission or treat it as a behavioral failure.

## Optional destinations and kits

`--destination /path/to/skills` overrides the install location. Within a Cargo
project it still resolves that application's dependency; outside one it explicitly
installs the invoked CLI's bundled skills. Both install and update accept it.

```sh
cargo rostfrei skills install --kit /path/to/extracted-kit --destination /path/to/skills
```

`--kit` bypasses dependency discovery and verifies the manifest and exact tool
compatibility; without `--destination` it uses the current directory's `.agents/skills`.
A different kit build requires its matching native tool already cached. Verify an
archive's `.sha256` companion with `sha256sum --check <archive>.sha256` before extraction.

Matching tools live under `$CARGO_HOME/rostfrei/tools` (default `$HOME/.cargo/rostfrei/tools`);
`ROSTFREI_TOOLS_DIR` overrides this cache. Keep it available in the execution environment:
test execution never downloads a tool. Direct `/path/to/cargo-rostfrei rostfrei ...`
invocation needs no Cargo, Git, Python or source checkout for execution or matching-kit setup.

## Framework source and packaging

The authoritative skills live in `.agents/skills`; Cargo packages flatten their
resource symlinks into regular files. From a symlink-preserving framework checkout,
use `cargo rostfrei-dev skills info` and `cargo rostfrei-dev test <definition.json>
--output <new-evidence.json>`. To install for a workspace app, use
`cargo rostfrei-dev skills install --package bike-rental`. Cargo rebuilds source edits.
Local builds carry source fingerprints; generated install output inside the source
workspace can affect that fingerprint. Use an external destination to avoid this feedback.

Maintainers can build, check and export the kit from the repository root:

```sh
cargo build --locked -p rostfrei-structure --bin cargo-rostfrei
python3 scripts/check_agent_skills.py
python3 scripts/check_agent_onboarding.py
python3 scripts/package_agent_skills.py --output /path/to/artifacts
```
