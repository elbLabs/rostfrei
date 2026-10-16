# Local runtime/host/skills candidate qualification

This is preparation for the 0.1 composition, **not a published 0.1 release**.
The current workspace version is `0.0.5-alpha`. Preserve it for this checkpoint
unless the release owner explicitly prepares another version. An archive named
`rostfrei-agent-skills-0.0.5-alpha.tar.gz` can identify a new local candidate via
its full revision; its label does not make it identical to the old published tag.

## Freeze and build one identity

Run from the exact clean combined candidate, with Rust/Cargo/Git/linker on PATH.
Use an external evidence directory; generated evidence inside the source can
change dirty-build provenance. The in-workspace source-fingerprint feedback
limitation remains deferred. Do not edit the freeze during qualification.

```bash
# Set this to your owned writable evidence parent, outside the checkout.
export QUALIFICATION_PARENT=/path/to/owned/writable/evidence
export NEW_EVIDENCE="$(mktemp -d "$QUALIFICATION_PARENT/rostfrei-01-qualification-XXXXXX")"
mkdir "$NEW_EVIDENCE/tmp"
export TMPDIR="$NEW_EVIDENCE/tmp"
git rev-parse HEAD > "$NEW_EVIDENCE/frozen-revision.txt"
git status --porcelain > "$NEW_EVIDENCE/frozen-status.txt"
python3 scripts/test_agent_skill_candidate.py
python3 scripts/check_agent_candidate.py --output "$NEW_EVIDENCE/matched"
export ROSTFREI_CLI="$NEW_EVIDENCE/matched/cli/bin/cargo-rostfrei"
export ROSTFREI_TOOLS_DIR="$NEW_EVIDENCE/matched/tools"
```

The builder refuses dirty source and records commands/exits. It:

- Snapshots full locked Cargo metadata/lockfiles and **every resolved `rostfrei*`
  package**, including source URL/revision or local manifest path. Same version
  with another revision/path fails. Workspace paths must belong to the freeze.
- Checks root, packaged-CLI and external-app locks for `rustls >=0.23.45`.
  Failure is reported to the sole lockfile owner; this script never repairs locks.
- Packages/verifies `rostfrei-structure`, installs it externally, removes its
  owned extraction, then queries native identity with empty PATH.
- Builds the actual-version kit/checksum and matches full clean CLI/kit identity:
  revision, dirty state, kit version, framework version, skill names and contracts
  (Catalog 1, behavior/MessageSeries 2, fixture 1).
- Copies the unchanged bike-rental example into a standalone consumer, expands
  workspace manifest inheritance, pins all framework dependencies to the exact
  local Git revision, seeds its lock from the candidate and explicitly resolves
  its standalone graph. No feature implementation is added.
- Performs dependency-aware `skills install` and `skills update` there, checks
  both installed releases and the independent cached runner, and builds both hosts.
- Records separate workspace/external host build commands, locks and executable
  SHA-256s. **The HTTP catalog/client cannot attest the host Git revision.**

`artifacts.json: passed` means artifact preparation only. Live execution and
fresh-session acceptance remain pending until their separate records exist.
The local Git URL is an intentional disposable-consumer pin, not a published pin.

## Required component and live checks

```bash
cargo test --locked -p rostfrei-structure
cargo clippy --locked -p rostfrei-structure --all-targets -- -D warnings
cargo fmt --all --check
python3 scripts/check_agent_skills.py
python3 scripts/check_agent_onboarding.py
python3 scripts/test_agent_skill_source.py
python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py \
  --require-handler-simulation --output "$NEW_EVIDENCE/installed-live"
python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py \
  --source-checkout --require-handler-simulation --output "$NEW_EVIDENCE/source-live"
python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py \
  --external-app "$NEW_EVIDENCE/matched/external-bike-rental" \
  --require-handler-simulation \
  --host-build-provenance "$NEW_EVIDENCE/matched/external-host-build.json" \
  --output "$NEW_EVIDENCE/external-live" \
  --host-command "$NEW_EVIDENCE/matched/external-target/debug/bike-rental-api" --tracer
```

Each live driver owns a unique app, fresh `ROSTFREI_COMPLETED_OUTCOME_EPOCH`,
loopback HTTP port and distinct ephemeral control/dispatch/inspection tokens.
`test_nats.py` owns/removes its disposable broker. The default host command is
`cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer`.
Source mode clones the **same commit**, not a synthetic same-content commit.

Evidence requires pass → intentional comparison mismatch (exit 1, complete exact
capture) → **unchanged** pass, with expired Test observations across repeated
resets. A disposable Normal command is committed before the resets; its observed
history stays unchanged after each, and a new duplicate command must still reject
after all three. This is not permission to dispatch to an existing application.
Never automatically resubmit an uncertain command/suite. Exit 2/incomplete
capture is blocked qualification, not a business red.

PR112 is selected: `--require-handler-simulation` is mandatory for this matrix.
It checks accepted/rejected proposals, no operation/receipt/publication-shaped
response or Location, unchanged history/observation and retired Preview HTTP 404.
All supported actions follow catalog/resource links. Only the unadvertised
retired route is named for the negative 404 check. HTTP evidence is paired with
the runtime owner's native no-publication/receipt tests; absence of an observed
event alone cannot prove absence of broker publication. The optional absence
mode is solely for genuinely older unrelated releases and is not combined coverage.

Keep the existing onboarding retention test: global A installs and retains A,
global B replaces the invoked binary, then empty-PATH execution routes to cached
A after owned source removal. Its intentional configuration sentinel proves
routing, not live NATS behavior. Installed live execution separately removes its
owned extracted kit before the native empty-PATH suite runs. Preserve all existing
wrong-tool, collision, edited-file, uncertainty and stale-source controls.

Record final command/exits, source status, artifact hashes and all failures outside
the checkout. Runtime/host native boundary, codecs, reset failure/recovery, TLS,
Studio stale-control and full integration acceptance belong to their owners.
Do not infer them from these HTTP/packaging results.

## Parent-owned fresh-session trial setup

After the freeze and preceding qualification, prepare **another** external app:

```bash
export TRIAL_APP="$(mktemp -d "$QUALIFICATION_PARENT/rostfrei-01-trial-parent-XXXXXX")/app"
python3 scripts/check_agent_candidate.py --prepare-app "$TRIAL_APP"
# Run these commands in TRIAL_APP (or supply --manifest-path where supported):
cd "$TRIAL_APP"
cargo metadata --format-version 1 > ../resolved-before-setup.json
cargo metadata --locked --format-version 1 > ../resolved-locked.json
"$ROSTFREI_CLI" rostfrei skills install
"$ROSTFREI_CLI" rostfrei skills update
cargo build --locked -p bike-rental --bin bike-rental-api
git init --quiet
git add .
git -c commit.gpgsign=false commit -m 'Frozen external bike-rental trial baseline'
```

Save the manifest/lock, both installed `references/release.json` files, managed
baseline, CLI identity and cached runner separately. Check every resolved
Rostfrei revision against `frozen-revision.txt`. Set up the runtime in the parent
shell (Bash, tracing disabled; variables are not printed or stored):

```bash
set +x
export OWNED_BROKER="rostfrei-trial-$(cat /proc/sys/kernel/random/uuid)"
docker run --rm --detach --name "$OWNED_BROKER" \
  -p 127.0.0.1::4222 nats:2.12.15-alpine -js
export ROSTFREI_NATS_URL="nats://$(docker port "$OWNED_BROKER" 4222/tcp)"
export ROSTFREI_APPLICATION="trial-$(cat /proc/sys/kernel/random/uuid)"
export ROSTFREI_COMPLETED_OUTCOME_EPOCH="trial-$(cat /proc/sys/kernel/random/uuid)"
export ROSTFREI_API_TOKEN="$(openssl rand -hex 24)"
export ROSTFREI_DISPATCH_TOKEN="$(openssl rand -hex 24)"
export ROSTFREI_INSPECTION_TOKEN="$(openssl rand -hex 24)"
export ROSTFREI_API_ADDR="127.0.0.1:$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
export ROSTFREI_TRACER_URL="http://$ROSTFREI_API_ADDR"
cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer
```

The last command runs in the foreground; the parent starts the fresh agent in
the external app with the same scoped environment. The new session discovers
only its installed skills. Supply scenario **B** from
`agent-skills-qualification.md` after the human confirms the still-missing rule.
Do not supply implementation guidance, scripted solutions, or the documented
holdout. This implementer has not conducted the trial or used the holdout.

Use that document's record/rubric: timestamped test-before-code diffs and
explanation, real behavioral red, stable-expectation green, checks, fixture and
capture identity, client/model version, duration/tokens if available, actual human
comments and misunderstandings. Mark each item observed-pass/fail, blocked or
unexecuted. **Human feedback remains pending until a human responds.**
Stop the foreground host, `docker stop "$OWNED_BROKER"`, and unset capability
variables. Retain only non-secret evidence; do not hand off live tokens/processes.

## Migration/release handoff

- Carry the complete reviewed runtime prerequisites, durable completed outcomes,
  typed failures/redrive, actor propagation, split authority and lifecycle fencing.
- Shared hosts use execution-store bindings, retained registrations/dependencies
  and effective shared codecs across fixtures, dispatch and diagnostics.
- Fresh authority selection is explicit. Legacy reads remain read-only; Test reset
  replaces both authoritative streams, reconnects and rebuilds bound services while
  retaining Normal. Existing-data migration is outside this checkpoint.
- Handler-only Simulate is diagnostic, uncommitted and operation-free. Preview
  submission is retired; real Test remains the behavior acceptance path. Record
  any remaining direct-runtime-reset diagnostic-health limitation explicitly.
- Skills install/update is explicit and dependency-matched; a newer global setup
  binary does not upgrade an app implicitly. The connection-only published starter
  remains separately gated; see [starter follow-up](starter-skills-followup.md).
- Keep the actual pre-release version in local assets. Future version/lock/changelog
  preparation, tag, draft workflow, attachment verification and publication each
  require the release owner's authorization. No workflow is dispatched here.

The integration owner should consolidate these notes into the root release notes,
keep the required-capability live commands in CI, and document the explicit epoch
in root/example host startup instructions. The same owner verifies the final
locked package/CLI TLS tree is patched to at least `rustls 0.23.45` and carries the
existing TLS checks. Fast Inbox rehearsal is conditional on availability; existing
applications/Nexus and further performance work are not added to this scope.
