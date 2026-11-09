# Repository tooling

`cargo xtask` runs the unpublished Rust workspace tool. It replaces the former
repository scripts; development and CI require no Python interpreter or packages.

```sh
cargo xtask --help
cargo test --locked -p xtask
cargo xtask versions check
cargo xtask versions bump 0.1.3
cargo xtask test
cargo xtask test -- cargo xtask acceptance tracer-host --working-tree
cargo xtask test -- cargo xtask acceptance bike-rental-host --studio
cargo xtask acceptance new-project
cargo xtask acceptance skills
cargo xtask acceptance onboarding
cargo xtask acceptance source
cargo xtask skills validate .agents/skills/rostfrei-tracer .agents/skills/rostfrei-develop-feature
cargo xtask skills package --output /path/to/artifacts
cargo xtask skills candidate --output /path/to/new-external-evidence
cargo xtask test -- cargo xtask skills live --require-handler-simulation --output /path/to/new-live-evidence
```

The default test command prefetches the deliberately offline macro fixture and
starts authenticated NATS 2.12.15 in Docker. Set `ROSTFREI_NATS_SERVER` to a cached
native server for restart/fault tests. An explicit command uses the ordinary
disposable broker unless `--contracts` is selected. Inherited broker addresses
are always replaced. Child processes have owned process groups; cancellation,
timeouts and normal exit terminate descendants and clean up the owned broker.
Secrets are redacted from both output streams, including fragmented output.

`cargo xtask contracts --nats-binary /path/to/nats-server --evidence-dir /path/to/evidence -- COMMAND`
uses an authenticated native broker instead of Docker and retains its private
configuration, storage, command log and monitoring snapshots. The repository's
pinned toolchain is selected by Cargo. Evidence directories are private.

Build `cargo build --locked -p rostfrei-structure --bin cargo-rostfrei` before
skill acceptance/packaging, or set `ROSTFREI_CLI` to a matching installed binary.
Candidate and live release qualification require a clean checkout and retain
command, identity and HTTP evidence. `acceptance source` and `acceptance onboarding`
exercise fresh temporary Git repositories and real Cargo resolution; they can
take several minutes and require cached dependencies or registry/Git access.
Source fixtures disable background Git maintenance before committing.

## Skill format validation

The native validator follows the Agent Skills reference rules at
[`69ef37e9424c0a7ea9dd2293b559e43ec8176379`](https://github.com/agentskills/agentskills/tree/69ef37e9424c0a7ea9dd2293b559e43ec8176379/skills-ref):
strict YAML frontmatter, allowed fields, Unicode-normalized lowercase names,
directory matching and description/compatibility length limits. Rust conformance
cases in `tests/skills.rs` cover accepted and rejected documents. This is a native
implementation of those rules; it no longer downloads or executes the upstream
validator. Installed-kit acceptance also validates local links, release metadata,
licenses, archive integrity, relocation and preservation of consumer edits.

## Migration map

| Former script responsibility | Rust entry point |
| --- | --- |
| NATS test runner and authenticated native runner | `test`, `contracts` |
| Version checks, bumps and rollback | `versions check`, `versions bump` |
| Generated public starter acceptance | `acceptance new-project` |
| Standalone Tracer/application/Studio acceptance | `acceptance tracer-host`, `acceptance bike-rental-host` |
| Skill packaging and reference-format validation | `skills package`, `skills validate` |
| Installed kits, Cargo onboarding and source freshness | `acceptance skills`, `acceptance onboarding`, `acceptance source` |
| Matched candidate preparation and live HTTP/SSE checks | `skills candidate`, `skills live` |
| Runner, version, candidate and HTTP regression tests | `cargo test --locked -p xtask` |

Historical acceptance reports retain their original commands and results. Use
the commands above when reproducing those checks with the current checkout.
