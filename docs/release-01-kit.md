# Skills candidate qualification

Use the actual workspace version and one clean Git revision for all artifacts.
An archive's version label alone does not identify its source or imply publication.

## Build one matched candidate

Start from a clean committed checkout with Rust/Cargo/Git/linker available. Keep
that source unchanged and all evidence outside it to avoid fingerprint feedback.

```bash
export QUALIFICATION_PARENT=/path/to/owned/writable/evidence
export NEW_EVIDENCE="$(mktemp -d "$QUALIFICATION_PARENT/rostfrei-qualification-XXXXXX")"
mkdir "$NEW_EVIDENCE/tmp"
export TMPDIR="$NEW_EVIDENCE/tmp"
git rev-parse HEAD > "$NEW_EVIDENCE/frozen-revision.txt"
git status --porcelain > "$NEW_EVIDENCE/frozen-status.txt"
python3 scripts/check_agent_candidate.py --output "$NEW_EVIDENCE/matched"
export ROSTFREI_CLI="$NEW_EVIDENCE/matched/cli/bin/cargo-rostfrei"
export ROSTFREI_TOOLS_DIR="$NEW_EVIDENCE/matched/tools"
```

The builder records commands/exits, locked metadata and every resolved `rostfrei*`
source/revision. It checks `rustls >=0.23.45` in workspace, packaged-CLI and external
app locks; installs the packaged CLI externally; removes its extracted source; and
matches clean CLI, kit/checksum, installed skills and cached-runner identities.
The external bike-rental app pins every framework dependency to the same local Git
commit. Workspace/external host records include build commands, locks and binary
hashes: **the HTTP catalog cannot attest a host's Git revision**.
`artifacts.json: passed` records preparation, not live or feature acceptance.

## Component and three-mode live checks

```bash
python3 scripts/test_agent_skill_candidate.py
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

Each live mode uses a unique app, fresh `ROSTFREI_COMPLETED_OUTCOME_EPOCH`, loopback
port and distinct control/dispatch/inspection tokens; `test_nats.py` removes its
disposable broker. Source mode clones the exact commit. Installed mode removes the
kit before empty-tools-PATH execution; onboarding also checks retained-runner routing.
The epoch explicitly selects fresh split authority; existing legacy history remains
read-only without a fenced cutover. Test reset recreates both journal/outcome-state
streams, reconnects and rebuilds services, retaining Normal resources.

Require pass → intentional comparison mismatch (exit 1) → unchanged pass, each with
complete, settled, exact captures. Repeated resets must expire Test observations,
preserve the seeded Normal history/observation and leave a new duplicate command
rejected. Exit 2/incomplete retrieval blocks qualification. Never automatically retry
an uncertain command or suite submission; the comparison control is not feature red.
Required handler checks cover accepted/rejected uncommitted proposals, no operation,
receipt/publication-shaped result or Location, unchanged history and retired Preview
HTTP 404. Native no-publication/receipt tests remain necessary; HTTP silence alone
cannot establish that nothing was published.

## Fresh installed-skill feature trial

```bash
export TRIAL_APP="$(mktemp -d "$QUALIFICATION_PARENT/rostfrei-trial-XXXXXX")/app"
python3 scripts/check_agent_candidate.py --prepare-app "$TRIAL_APP"
cp clippy.toml "$TRIAL_APP/clippy.toml" # The helper omits the root Clippy policy.
```

Resolve its standalone lock, run skills install/update with `$ROSTFREI_CLI`, and
record the manifest/lock, both installed releases, managed baseline and cached runner.
Build the host and save its command/binary hash. Start it with `--tracer` on an owned
NATS broker, unique application and fresh epoch, using distinct ephemeral tokens and
a loopback endpoint. Start a fresh agent in that app with its installed skills.

Agree on a genuinely missing behavior with the human. Record the test-before-code
diff and explanation, real NATS behavioral red, implementation, then green with
stable expectations, then run application checks. Retain fixture/capture identity,
command exits, source diffs, client/model version and actual human comments or
misunderstandings. Mark results observed-pass/fail, blocked or unexecuted; record
feedback only when received.
Stop the owned host/broker and remove credentials; retain non-secret evidence.

**Release:** Publish the qualified version/revision with verified artifacts; the published
starter's skills capability has a separate [post-publication follow-up](starter-skills-followup.md).
