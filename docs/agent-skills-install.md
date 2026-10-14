# Rostfrei Agent Skills kit

The kit contains **rostfrei-develop-feature** and **rostfrei-tracer**, their
relative resources, compatibility metadata and file hashes. Installation and
execution use **Rostfrei's Rust CLI**, `cargo-rostfrei`; no consumer Python is
required or shipped. Running application acceptance needs a matching
application-linked Tracer with real isolated NATS.

## Install from a release

Download `rostfrei-agent-skills-0.1.0.tar.gz` and its `.sha256` companion from the
matching Rostfrei release. Verify and extract in the download directory:

These filenames illustrate the upcoming 0.1.0 artifact. Use the downloaded
release's version; the current source baseline builds `0.0.5-alpha` until the
shared release version is bumped.

```sh
sha256sum --check rostfrei-agent-skills-0.1.0.tar.gz.sha256
tar -xzf rostfrei-agent-skills-0.1.0.tar.gz
cd rostfrei-agent-skills-0.1.0
```

Read `sourceRevision` from the extracted `release.json`, then install that exact
CLI revision (not the older published alpha, which predates these commands):

```sh
cargo install --locked --git https://github.com/elbLabs/rostfrei \
  --rev <sourceRevision> rostfrei-structure
cargo rostfrei skills info
cargo rostfrei skills install --kit "$PWD" \
  --destination /absolute/path/to/application/.agents/skills
```

Building requires Rust 1.98+, Cargo, Git, a native linker and registry/Git network
access. The kit is platform-neutral; it contains no prebuilt native binary.
Cargo builds the executable for the consumer platform. A matching source
checkout also supports `cargo install --locked --path crates/rostfrei-structure`.
The installed binary needs neither Python nor Git nor the source checkout, and
does not contact package registries at runtime. Direct invocation, even without
Cargo on PATH: `/path/to/cargo-rostfrei rostfrei skills install --kit <kit>
--destination <skills-directory>`.

The destination is the **skills directory**, not the application root. The
installer requires an explicit destination, checks the extracted kit's manifest
and exact CLI/framework revision compatibility,
copies both skills, and preserves unrelated files. Identical installs are no-ops.
Any differing existing skill (including extra consumer files) stops the whole
preflight before any skill is installed. There is no overwrite switch: install
an upgrade into a separate directory and deliberately merge local changes.
Unrelated `AGENTS.md`, client configuration and other skills are never edited.

The extracted kit and source checkout can be removed after installation. Every
installed skill includes `references/release.json` recording framework version,
source revision, dirty-build status and protocol compatibility. The archive's
version comes from `workspace.package.version` in the release's Cargo manifest.
Use a clean release artifact, and match the consuming application's dependency
revision; equal `0.1.0` version labels alone do not imply equal prerelease code.

## Client discovery

These **project-local locations** were checked against the linked authoritative
documentation on 2026-10-13:

| Client | Explicit installer destination | Documentation |
| --- | --- | --- |
| OpenCode V2 | `/path/to/app/.opencode/skills` or `/path/to/app/.agents/skills` | [V2 skills discovery](https://opencode.ai/v2/docs/skills) |
| Codex CLI / IDE | `/path/to/app/.agents/skills` | [Where Codex loads local skills](https://developers.openai.com/codex/build-skills) |

`.agents/skills` is the shared choice. Install once per project; avoid duplicate
IDs across discovery locations. Launch the client in the application repository.
Explicitly request `rostfrei-develop-feature` for a feature or `rostfrei-tracer`
for inspection. A missing host question tool falls back to ordinary conversation.
Documented discovery is not proof of workflow quality; fresh-session trials are
tracked in the qualification guide. This installer does not change user/global
configuration.

## Start and run

Use the application's documented binary exposing registered fixtures and the
behavior suite. The current framework example is the actual `bike-rental-api`
binary. In a matching framework checkout, with Rust, Docker and distinct control
and dispatch tokens supplied in the environment:

```sh
# Terminal 1: owned disposable broker, no persistent volume
docker run --rm --name rostfrei-skills-nats -p 127.0.0.1:14222:4222 nats:2.12.1-alpine -js
# Terminal 2: matching framework checkout
ROSTFREI_NATS_URL=nats://127.0.0.1:14222 \
  cargo run --locked -p bike-rental --bin bike-rental-api
```

Stop the host and `docker stop rostfrei-skills-nats` after use.
The host defaults to `127.0.0.1:1309` (`ROSTFREI_API_ADDR` overrides it). It
provisions both Test and Normal topology and resets Test at startup. Never point
it at an unknown shared namespace. A real consumer supplies its own application
composition, handlers and external dependency fixtures.

The installed develop-feature skill includes a complete JSON v2 definition.
In another terminal with `ROSTFREI_TRACER_URL` and
`ROSTFREI_API_TOKEN` available, after authorizing this specific isolated scope:

```sh
SKILL=/absolute/path/to/application/.agents/skills/rostfrei-develop-feature
cargo rostfrei test "$SKILL/assets/add-first-bicycle.json" --output behavior-evidence.json
```

For application-owned definitions, add `--release "$SKILL/references/release.json"`.
The binary can also run directly as `/path/to/cargo-rostfrei rostfrei test ...`.
Exit 0 means reported pass, 1 means reported failure, and 2 means typed
blocked/incomplete execution. Complete verdicts require successful evidence
retrieval; later capture failure preserves the report and exits 2. Evidence
files are created exclusively and submissions are never automatically retried.

This runs the real NATS-backed suite; Preview is prediction only. Each run resets
the host's entire configured Test scenario, not just the command's aggregate.
See the installed skills for reset impact, capability separation and evidence
interpretation. No source checkout is required by the installed Rust client.

**Release integration still required:** #107's standard application-linked host
uses `cargo run --locked -p bike-rental -- --tracer` at its separate revision.
Final combined installed-kit 0.1.0 and fresh-session rehearsals remain gates.
The actual contracts are Catalog 1, behavior/MessageSeries 2 and fixture 1;
the changelog records the intentional Catalog v2 → v1 change.

## Repository maintenance

The only authoritative skill sources are `.agents/skills/<name>/`. This is
discovered directly by OpenCode V2 and Codex, without symlinks, duplicate copies
or configuration. The earlier `.opencode/skills/rostfrei-tracer` source moved
there. Use source-discovered skills directly from the framework root:

```sh
cargo rostfrei-dev skills info
cargo rostfrei-dev test .agents/skills/rostfrei-develop-feature/assets/add-first-bicycle.json \
  --output source-evidence.json
```

Use the documented isolated runtime and authorization scope above. There is no
source bootstrap or generated-file dependency. Cargo refreshes compiled revision
metadata when HEAD changes; the schema comes from the live catalog. Legacy
generated metadata, if present beside an example, is validated and preserved:
move a stale file aside deliberately or use the matching CLI. Installed artifacts
bundle release metadata, runtime-schema snapshot and licenses.

From the framework repository:

```sh
cargo build --locked -p rostfrei-structure --bin cargo-rostfrei
python3 scripts/check_agent_skills.py
python3 scripts/package_agent_skills.py --output /path/to/artifacts
```

Python remains repository-only packaging/CI infrastructure. The builder uses the
Rust CLI's release metadata and the runtime's authoritative schema; only tracked
Markdown/JSON/license skill resources are packaged, excluding local caches and
untracked work. `ROSTFREI_CLI` can select an installed matching native executable.

The check builds and installs into a standalone temporary consumer outside the
workspace and validates both skills with the pinned upstream reference validator.
`scripts/skills_ref_validator.py` downloads SHA-256-pinned source at Agent Skills
revision `69ef37e9424c0a7ea9dd2293b559e43ec8176379`, StrictYAML 1.7.3,
python-dateutil 2.9.0.post0 and six 1.17.0 into disposable storage. It calls the
reference library directly, needing no pip/venv, build backend, system package
installation or host-specific tool. CI requires HTTPS access to codeload GitHub
and files.pythonhosted.org; checksum/network errors fail the check.

The existing prepare-release workflow validates, builds and attaches the archive
and checksum to a **draft** release. It does not publish the release.
