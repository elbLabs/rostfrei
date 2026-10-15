# Explicit Rostfrei skills setup

Rostfrei is a **Cargo library dependency**. Adding it does not install a global
command, and neither building nor starting an application writes skills. Install
the Rust setup tool once, then deliberately set up each application:

```sh
# Once, after this setup feature is merged into the selected Git source:
cargo install --locked --git https://github.com/elbLabs/rostfrei rostfrei-structure
# Inside an existing application with its dependency resolved in Cargo.lock:
cargo rostfrei skills install
# After deliberately upgrading the application's dependency and lockfile:
cargo rostfrei skills update
```

**PR #106 is not yet merged/published.** To try this branch now, install the tool
from its exact reviewed commit with `--rev <PR-sourceRevision>`, or from its
checkout using `cargo install --locked --path crates/rostfrei-structure`.
The previously published `v0.0.5-alpha` framework revision predates this setup
capability. Selecting it reports the real missing capability; the new setup tool
never substitutes its own newer skills for that application's older dependency.

Initial tool compilation needs Rust 1.98+, Cargo, Git, a native linker and access
to the selected Git/registry source. No consumer Python is required. Library
versions and tool installation remain separate; a newer setup tool can serve
multiple applications without replacing a global binary for every dependency.

## What installation resolves

The command runs `cargo metadata --locked`, using the actual resolved package
graph, including renamed and inherited dependencies. It selects a unique
reachable package named `rostfrei`, its exact version and source revision. Create
or update the lockfile through Cargo first if it does not exist or is stale.

The default destination is the selected application's `.agents/skills`, next to
its `Cargo.toml`. A single default workspace member is supported. If multiple
applications are possible, select deliberately:

```sh
cargo rostfrei skills install --manifest-path /path/to/workspace/Cargo.toml --package my-app
cargo rostfrei skills install --destination /path/to/client/skills
```

No dependency, multiple reachable framework revisions, unknown source provenance
or unsupported source types produce a diagnostic before skill installation.
Crates.io packages require their published Cargo VCS metadata; Git dependencies
use Cargo's resolved full commit. Git-backed local path dependencies use that
checkout's exact revision and matching sibling `rostfrei-structure` source.
Local sources are rebuilt during explicit setup so uncommitted resource changes
cannot be hidden by a revision-keyed cache; their dirty-build provenance is retained.

If the installed setup CLI already matches, it uses its bundled resources.
Otherwise it obtains `rostfrei-structure` from the **same exact Git source/commit**,
exact crates.io version, or matching local source. Cargo handles retrieval/build;
there are no invented release-download URLs or `latest` substitutions. The tool's
reported version/revision is checked before use. Old releases without bundled
setup support fail explicitly. Acquisition may need network access on first use.

Matching native tools are cached under `$CARGO_HOME/rostfrei/tools` (or
`$HOME/.cargo/rostfrei/tools`), keyed by version, revision and platform.
`ROSTFREI_TOOLS_DIR` overrides this cache root. Keep that cache available when
moving between setup and execution environments. It is not committed to the app.

## Explicit updates and consumer work

`skills update` follows the **current application dependency**, not an independent
latest-skills channel. `.agents/skills/.rostfrei-managed.json` is deterministic,
commit-able metadata recording the installed release/protocol provenance and
managed file hashes. Commit it alongside the installed skill directories.

Install/update are idempotent. An ordinary install does not upgrade an existing
managed build; it asks you to use `skills update`. Update replaces unchanged
managed files, adds upstream resources and removes obsolete managed resources.
User-added files and unrelated skills/instructions are preserved. Edited managed
files produce conflicts with paths; resolve those deliberately and rerun. All
skills are preflighted together, staged, and committed with rollback on write or
rename failure. A conflict does not partially update the other skill.

Older installations without a managed baseline are adopted only when complete
skills are byte-identical. Different/unowned contents are preserved, not assumed
to be pristine. Install into another directory and deliberately merge if needed.

## Optional offline/standalone paths

Both flags remain independent overrides:

```sh
# No Cargo project: install this native CLI's bundled skills at an explicit path.
cargo rostfrei skills install --destination /path/to/consumer/.agents/skills
# Optional external kit import; destination defaults to cwd/.agents/skills.
cargo rostfrei skills install --kit /path/to/extracted-kit
cargo rostfrei skills install --kit /path/to/extracted-kit --destination /path/to/skills
```

The same overrides work with `skills update`. An explicit kit overrides dependency
discovery and undergoes full manifest and exact native-tool compatibility checks.
A different kit revision requires its already-cached matching tool. No Cargo,
Git, Python, source checkout or network is required for matching native/explicit
kit installation. Direct invocation works with an empty tools PATH:
`/path/to/cargo-rostfrei rostfrei skills install --destination /path/to/skills`.
Within a Cargo project, `--destination` alone still resolves its dependency.

Optional release archives are `rostfrei-agent-skills-<version>.tar.gz` plus a
`.sha256` companion. Verify with `sha256sum --check <archive>.sha256` and extract
before `--kit` import. Archive versions follow `workspace.package.version`,
currently **0.0.5-alpha**; **0.1.0** is the release milestone. Ordinary setup does
not require an archive.

## Client discovery and execution

| Client | Project-local discovery directory | Authoritative documentation |
| --- | --- | --- |
| OpenCode V2 | `.agents/skills` or `.opencode/skills` | [V2 discovery](https://opencode.ai/v2/docs/skills) |
| Codex CLI / IDE | `.agents/skills` | [Codex skills](https://developers.openai.com/codex/build-skills) |

These locations were checked on 2026-10-13. Install once per project, avoid duplicate
skill IDs, and launch the client in the app. No user/global client configuration
is changed. Ordinary conversation substitutes for host-specific question tools.

With the authorized isolated runtime's `ROSTFREI_TRACER_URL` and
`ROSTFREI_API_TOKEN` in the environment:

```sh
cargo rostfrei test .agents/skills/rostfrei-develop-feature/assets/add-first-bicycle.json \
  --output behavior-evidence.json
# Application-owned definition: select the installed release explicitly.
cargo rostfrei test tests/my-behavior.json --output my-evidence.json \
  --release .agents/skills/rostfrei-develop-feature/references/release.json
```

The runner selects the installed release's already-cached native CLI if needed;
exact revision/protocol checks remain enforced. Execution performs no tool
downloads and needs neither Cargo/Git/Python nor the source checkout when invoked
directly. Exit 0/1 means a supported reported pass/fail with retrieved evidence;
exit 2 means typed blocked/incomplete execution. Uncertain submissions are never
retried and existing evidence is never overwritten. Later capture failure retains
the complete report/verdict. Preview is prediction, not acceptance.

Use the application's actual handlers, workers, fixtures and NATS event store on
an authorized disposable NATS 2.12.1+ broker with JetStream. The verified baseline
host is `cargo run --locked -p bike-rental --bin bike-rental-api`; supply
`ROSTFREI_NATS_URL` and distinct control/dispatch tokens. Default bind is
`127.0.0.1:1309`. Runs reset the entire configured Test scenario. See each skill's
runtime reference for isolation, reset scope and broker cleanup.

## New-project release blocker

`cargo rostfrei new my-app` still pins the **published** `v0.0.5-alpha` revision
`c9f5516ddc324265916b60e1eb7300dad9acac9d`. That release predates skills-enabled
tooling. The command preserves its working scaffold and explicitly reports that
skills were not installed; attaching the current bundle would be incompatible.
The shared setup hook runs inside staged/atomic project creation once the starter
pin advances to a genuinely published skills-enabled release and its capability
is qualified. This PR does not publish a release or invent a 0.1.0 pin.

## Source and package maintenance

Source discovery stays at the sole authoritative `.agents/skills/<name>` tree.
The CLI's explicitly enumerated `resources/` entries are Git symlinks to those
files, the repository license, and the existing runtime-generated schema. Git/path
builds need a symlink-preserving checkout. Cargo **flattens these references into
regular files in the `.crate`**; packaged builds need no external resources or
symlink support. There is no manually synchronized second content/schema source.
The native bundle supplies both installation and archive export. Release metadata
comes from the CLI build, and protocol numbers remain Catalog 1, behavior and
MessageSeries 2, fixture 1.

From this checkout, use `cargo rostfrei-dev skills install --package bike-rental`
for the workspace application (or `--manifest-path` for another app), and
`cargo rostfrei-dev test <definition.json> --output <new-evidence.json>` for source
execution. No source preparation command is required. Source resource edits are
picked up by an ordinary Rust rebuild and reflected in dirty-build provenance.

Repository-only maintenance/CI commands:

```sh
cargo build --locked -p rostfrei-structure --bin cargo-rostfrei
python3 scripts/check_agent_skills.py
python3 scripts/check_agent_onboarding.py
python3 scripts/package_agent_skills.py --output /path/to/artifacts
```

The exporter validates native bundled bytes against tracked authoritative inputs;
ignored/untracked junk is excluded. Tests install an independently verified Cargo
package outside the workspace, remove its source, validate installed skills using
the pinned upstream Agent Skills validator, exercise dependency upgrades, and run
real NATS controls. The draft-release workflow attaches the archive/checksum but
does not publish. Final combined #107 host qualification, fresh-session trials with
human feedback and final release rehearsal remain explicit release gates.
