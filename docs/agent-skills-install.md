# Rostfrei Agent Skills kit

The kit contains **rostfrei-develop-feature** and **rostfrei-tracer**, their
relative resources, a Python installer, compatibility metadata and file hashes.
Installation needs only Python 3.11+. Running application acceptance additionally
needs a matching application-linked Tracer with real isolated NATS; see each
skill's execution prerequisites.

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
python3 install.py --destination /absolute/path/to/application/.agents/skills
```

The destination is the **skills directory**, not the application root. The
installer requires an explicit destination, checks the extracted kit's manifest,
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
python3 scripts/test_nats.py -- cargo run --locked -p bike-rental --bin bike-rental-api
```

The runner creates a disposable pinned NATS broker and removes it on exit.
The host defaults to `127.0.0.1:1309` (`ROSTFREI_API_ADDR` overrides it). It
provisions both Test and Normal topology and resets Test at startup. Never point
it at an unknown shared namespace. A real consumer supplies its own application
composition, handlers and external dependency fixtures.

The installed develop-feature skill includes a complete JSON v2 definition and
a small Python HTTP runner. In another terminal with `ROSTFREI_TRACER_URL` and
`ROSTFREI_API_TOKEN` available, after authorizing this specific isolated scope:

```sh
SKILL=/absolute/path/to/application/.agents/skills/rostfrei-develop-feature
python3 "$SKILL/scripts/run_behavior.py" \
  "$SKILL/assets/add-first-bicycle.json" --output behavior-evidence.json
```

This runs the real NATS-backed suite; Preview is prediction only. Each run resets
the host's entire configured Test scenario, not just the command's aggregate.
See the installed skills for reset impact, capability separation and evidence
interpretation. No source checkout is required by the installed client script.

**Release integration still required:** #69's standard application-linked host
API/startup command and the combined installed-kit 0.1.0 rehearsal. At the
specified #105 base, the implemented Catalog is v1 and behavior definitions are
v2; the issue's Catalog v2 expectation needs reconciliation against the final
runtime. The kit records the actual source contract rather than relabeling it.

## Repository maintenance

The only authoritative skill sources are `.agents/skills/<name>/`. This is
discovered directly by OpenCode V2 and Codex, without symlinks, duplicate copies
or configuration. The earlier `.opencode/skills/rostfrei-tracer` source moved
there. Before using a directly discovered source skill, prepare its generated
resources from the framework root (Python 3.11+ and Git):

```sh
python3 scripts/package_agent_skills.py --prepare-source
SKILL="$PWD/.agents/skills/rostfrei-develop-feature"
python3 "$SKILL/scripts/run_behavior.py" \
  "$SKILL/assets/add-first-bicycle.json" --output behavior-evidence.json
```

Use the documented isolated runtime and authorization scope above. Preparation
creates the relative release metadata, schema and licenses using the **same
generator as the archive builder**. Repeat it before reading resources after a
revision/input change; the source runner also prepares automatically each run.
These generated files and their ownership hashes are Git-ignored. Modified or
unowned conflicting resources stop preparation before writes: move your edited
file aside deliberately before regenerating. Skill instructions are never
overwritten. There is no second hand-maintained metadata/schema copy or duplicate
discovery tree. Installed artifacts need no source paths, Git or preparation.

From the framework repository:

```sh
python3 scripts/check_agent_skills.py
python3 scripts/package_agent_skills.py --output /path/to/artifacts
```

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
