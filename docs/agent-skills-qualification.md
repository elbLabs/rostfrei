# Agent Skills qualification — issue #105

## Combined runtime candidate checkpoint (2026-10-16)

Earlier executed records below are historical component/PR-only evidence, not
proof of the complete modular-runtime composition. The selected composition
requires handler diagnostics, explicit fresh split authority, repeated Test reset
with Normal preservation and one clean identity across app, native CLI, kit and
external dependencies. The runnable matrix and separate host provenance are in
[release-01-kit.md](release-01-kit.md). Component tooling checks are distinct from
that still-pending final-candidate execution.

The parent owns the fresh installed-skill scenario B after freeze. Its real
behavioral red/green and human feedback remain pending; scripted comparison
controls do not satisfy that acceptance. The documented holdout remains unused.

## Scope and evidence

**Current integration:** #106's real-execution skills have been qualified together
with #107's shared host and #112's legacy Preview retirement / explicit handler
diagnostics in a clean isolated snapshot. Historical records below retain their
original behavior; current capability evidence is recorded in the harmony guide.
See [exact revisions, commands and evidence](agent-skills-harmony.md). Installed,
fresh-source and pinned external-consumer paths each passed actual NATS
pass/mismatch/pass controls; scoped observation and quarantine were exercised.
Historical sections below retain the evidence available at each earlier stage.
Independent installed-skill agent competence/human feedback and final release
rehearsal remain unexecuted.

Framework/application baseline: `1d7d86153d0dc96d461454a26902b14caf737d39`,
Rostfrei workspace version **0.0.5-alpha** (target milestone 0.1.0); bike-rental
application from that same revision. The kit follows the existing version source;
the eventual 0.1.0 release-version bump is separate release preparation.
Implementation branch: `feat/agent-skills-kit`. Agent/client: GPT-6 Astra in
OpenCode V2. This implementation session is not a fresh-session workflow trial.

### Bounded independent-review repairs (2026-10-15)

The five independently reproduced defects were specified in focused CLI/HTTP
acceptance tests before production changes (`12e2f57`). Actual reds:

1. Local setup replaced the immutable cached executable's bytes.
2. Directory permission assertion observed `0775`, expected `0700`.
3. Rooted-workspace setup selected the root package and reported no framework,
   ignoring the configured default app.
4. HTTP 200 `{}` capture returned exit 0 rather than 2 (one run submission).
5. A managed directory-to-file transition failed with `Could not stage skill resource`.

Repairs stay at those boundaries: dirty source content fingerprints and isolated
cache entries with direct/delegated identity checks; staged directory-mode
restoration; Cargo default/member selection; runtime-generated observed-v2 schema
validation plus typed capture-envelope validation; and preflight directory layout
reconciliation preserving consumer files/empty directories. No scaffold pin or
onboarding design change is included. Old dirty metadata without a fingerprint
is rejected rather than assumed to identify a unique build; clean immutable
release compatibility is retained.

Observed local green: **76 Rust tests** (including 9 HTTP/client boundary and 7
installer tests), **6 real-Cargo onboarding checks**, **4 source regressions**,
workspace all-target/all-feature clippy and version checks. Cache acceptance
retains the clean runner while creating two distinct dirty builds at the same
commit, and rejects substituted executables in direct/delegated selection.
The HTTP identity regression records zero runtime requests/submissions on
mismatch. Malformed captures retain either reported verdict, mark capture
incomplete and never repeat the single submission; valid empty captures and
truncated-body classification remain covered. Layout tests cover both directions
and atomic preservation on user-file, empty-directory and edited-managed collisions.

Package qualification passed with **77 `.crate` files** and two external builds
at different source locations. Both dirty-package builds reported the identical
fingerprint `84340e00b6ae9b2b8ae7aee176c13c2570dca7c497b0b3da2e920d26bc0dc941`;
the package sources were then removed. **7 external kit/reference checks** passed
using that installed executable. Real NATS 2.12.15 on disposable port **33268**
recorded pass → deliberate expectation failure → unchanged pass, with settled,
exact captures. Evidence: `/tmp/rostfrei-review-fixes-hno7f76k/live`; fixture
`no-rental-fleets` revision `1`, namespace `skills-fa2e4dc67fe74be280bc17f8f0548baa`.
Owned host/broker were cleaned up; consumer PATH excluded Python/Cargo/Git and
the extracted kit was removed before execution. This pre-commit package records
`12e2f57` plus dirty repair inputs. Final clean-head CI is linked on PR #106.

At this earlier repair stage, combined #107 qualification was still pending;
the later combined result is recorded above. Independent fresh-session human
trials and the published-starter release blocker remain separate gates.

### Explicit dependency-aware setup (2026-10-15)

Agreed scenario: an existing Cargo app resolves Rostfrei; explicit install selects
that dependency's skills, and explicit update follows a dependency upgrade while
preserving local edits. This CLI/filesystem behavior cannot be expressed by the
NATS JSON-v2 message-graph suite. External Cargo/CLI acceptance covers it; the
real-NATS installed/source checks continue to cover actual runner behavior.

Test-first commit `ac58f93`: `python3 scripts/check_agent_onboarding.py` first
resolved/locked the real renamed, inherited framework dependency, then failed
with CLI exit **2**, `--kit <KIT>` and `--destination <DESTINATION>` required.
This is an observed missing-setup capability red, not application behavioral red.

Implemented commands are `skills install`, `skills update`, `skills info` and
`test`. The setup tool is installed once separately from the library. Cargo
metadata/lock resolution selects exact Git revisions or published crates.io
version/VCS provenance; matching tools are obtained through Cargo and cached.
Local path source builds refresh during explicit setup. Cached runner routing
preserves exact release/contracts checks and performs no runtime acquisition.
Optional kit/destination overrides retain source-free native paths. Updates use
a committed baseline of resource hashes, all-skill conflict preflight and staged
rollback; consumer additions survive and obsolete unmodified resources are removed.

Authoritative skills stay in `.agents/skills`. Explicit package resource links
reference that source, runtime schema and license; Cargo flattens them into
regular `.crate` files. The native bundle is also the archive export source,
validated against tracked authoritative inputs. There is no consumer Python or
build/startup installation side effect.

Initial local green evidence:

- **72 Rust tests** passed, including 7 HTTP failure-boundary tests, 5 installer
  tests, staged transaction rollback and atomic scaffold setup failure.
- **5 external onboarding checks** passed: real renamed/inherited resolution,
  absent dependency, explicit workspace/package selection, and two controlled
  Git revisions. The setup CLI's own revision differed from both selected tools.
  Dependency upgrade required `update`; a local edit blocked all skills, then
  deliberate resolution allowed replacement/removal/addition while preserving
  user notes. Cached update worked offline; runtime routing worked without
  Cargo/Git/Python. Invalid release skill sets were rejected without delegation.
  An uncommitted local source edit was rebuilt on explicit update; that dirty
  cached build was rejected for a later immutable Git dependency at the same SHA.
- **4 source regressions** passed, including ordinary Rust rebuild picking up a
  canonical skill edit and retaining dirty provenance without generated files.
- The fifth external onboarding check exercised the actual published starter and
  confirmed the pre-kit capability diagnostic before any skills were installed.
- **7 external kit acceptance checks** passed with the native executable from an
  externally extracted `.crate`, after its build-source directory was removed.
  Both skills passed pinned reference validation. Optional override combinations,
  empty-tools-PATH installation, idempotency, preservation and reproducible
  archives excluding ignored/untracked junk passed.
- `cargo package --locked --offline --allow-dirty -p rostfrei-structure` built and
  verified **74 files**. All eight resource entries were regular package files.
  The package was separately installed with `cargo install --locked --offline
  --debug --path <external-package> --root /tmp/rostfrei-onboarding-cli`, then the
  package source was deleted. This local pre-commit build records `ac58f93` plus
  dirty implementation changes; clean-revision packaging is repeated by CI.
- Workspace all-target/all-feature clippy, formatting, domain structure, shared
  versions and diff checks passed.
- Installed native real-NATS evidence: `/tmp/rostfrei-onboarding-installed`,
  disposable NATS 2.12.1 port **33262**, actual `bike-rental-api` host, pass →
  intentional comparison failure → unchanged pass. Source discovery evidence:
  `/tmp/rostfrei-onboarding-source`, separate broker port **33264**, actual pass
  without generated skill resources. All four captures settled/exact (500ms).
  Both owned brokers/hosts were cleaned up. Consumer commands ran without
  Python/Cargo/Git on PATH; the installed run also removed the extracted kit.

These scripted NATS controls are not a fresh-session feature-TDD trial. Final
remote checks are tracked on PR #106, separately from the local evidence above.

**Release blocker:** scaffold release `v0.0.5-alpha` / `c9f5516` genuinely predates
skills support. `cargo rostfrei new` preserves that working published dependency
and reports the gap. Its shared setup hook is inside staged atomic creation,
ready for a qualified published skills-enabled pin; this PR does not attach a
mismatched current bundle or invent/publish a future release. The promised
new-project skills inclusion remains pending that publication/pin qualification.

At this earlier onboarding stage, combined #107 qualification was unexecuted;
it is now covered by the integration record above. Independent fresh-session/human
feedback, final external-consumer release rehearsal and draft-release attachment
verification remain unexecuted release gates.

### Rust consumer migration (2026-10-14)

The supported consumer commands are now `cargo rostfrei skills install`,
`cargo rostfrei skills info`, and `cargo rostfrei test`. The Python runner and
installer were removed. Python remains repository-only packaging/CI orchestration.
See [installation](agent-skills-install.md) for the exact matching-revision
`cargo install` path and Rust-only host/source-use commands.

Test-first commit `a30c461` records two actual capability reds:

- External extracted-kit install with an empty tools PATH failed with
  `unrecognized subcommand 'skills'` (child exit 2).
- Rust CLI fault-server acceptance failed with `unrecognized subcommand 'test'`
  (observed exit 2 instead of expected 0). An initial test compile error was
  corrected before recording this capability red.

These are missing-CLI plumbing reds, not application behavioral red. The Rust
client reuses the live discovery, validation, fixture, suite-run and finite
message-series HTTP API. The suite engine remains in `rostfrei-tracer`.

Source-discovered skills need no generated files: the Rust CLI identifies its
compiled framework revision and obtains the live schema. Cargo rebuilds metadata
after HEAD changes. Stale legacy metadata is preserved and rejected. The source
fixture's local Git maintenance isolation and aggressive-GC cleanup regression
from `60576f3` remain in place.

The CLI package includes its build-provenance logic and implementation modules;
it does not depend on files outside its Cargo package at build/runtime. A Cargo
package obtains revision metadata from `.cargo_vcs_info.json`, while Git builds
record their checkout revision. Both paths avoid a runtime Git/Python dependency.
Kit generation checks CLI revision against the checkout and takes the schema
from the authoritative runtime input. Archive resource selection now explicitly
uses tracked Markdown/JSON/licenses, excluding caches and untracked user work.
This narrow selection change is part of the user-directed Rust migration.

Rust migration checks observed locally:

| Check | Result |
| --- | --- |
| `cargo test --locked -p rostfrei-structure` | 69 passed, including 7 HTTP fault/boundary tests and 4 installer tests. |
| `python3 scripts/test_agent_skill_source.py` | 3 passed: fresh source build/revision refresh without generated resources, preserved stale metadata, isolated Git fixture maintenance. |
| `python3 scripts/check_agent_skills.py` with an externally installed `ROSTFREI_CLI` | 5 passed, including relocation, pinned reference validation, exact-revision compatibility, no-clobber/idempotency and deterministic intended resource selection. Consumer installation uses an empty tools PATH. |
| `cargo install --locked --offline --path crates/rostfrei-structure --debug --root /tmp/rostfrei-skills-rust-cli` | Native executable installed outside the workspace. |
| `cargo package --locked --offline --allow-dirty -p rostfrei-structure` | 63-file Cargo package built and verified independently of workspace resource paths. `--allow-dirty` was used only for pre-commit local qualification; CI packages the clean revision. |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `cargo fmt --all --check`, `cargo rostfrei-dev check --workspace`, `python3 scripts/versions.py check`, `git diff --check HEAD` | Passed. |
| `python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py --output /tmp/rostfrei-skills-rust-installed` | Passed on disposable NATS 2.12.1, port 33249, using the externally installed native CLI. |
| Same live command with `--source-checkout --output /tmp/rostfrei-skills-rust-source` | Passed on disposable NATS 2.12.1, port 33251: fresh source copy, locally built Rust CLI, no generated skill resources. |

Both live runs used an empty tools PATH for consumer commands (no Python, Cargo
or Git) and a separately running real application host. The installed case
removed the extracted kit and archive before execution. Its accepted command and
committed `bicycle-added` passed, a deliberately wrong expected payload failed
with exit 1, and the unchanged example passed after reset. All four captures
including source execution were settled/exact. Both owned brokers were cleaned
up. Local installed evidence records `a30c461` plus dirty implementation changes;
fresh-source evidence records its clean, disposable fixture commit.

CI additionally installs the verified `.crate` from a separate external build
directory, deletes that build source, then performs all consumer install/live
checks with the resulting native executable. This covers package include rules
and the absence of runtime source-checkout dependencies.

That package-install/source-removal path was also exercised locally with
`/tmp/rostfrei-skills-rust-packaged-cli/bin/cargo-rostfrei`. After deleting its
external build source, installed-kit real-NATS verification passed again on
port 33253 using the already-built baseline `target/debug/bike-rental-api` host
via the live driver's explicit `--host-command` option. Evidence is in
`/tmp/rostfrei-skills-rust-packaged`: pass → deliberate comparison failure →
unchanged pass, with consumer tools absent from PATH and kit/build source removed.

An exact-revision Git installation was also executed successfully with
`cargo install --locked --git https://github.com/elbLabs/rostfrei --rev
bbc688056af223c25fcd15be2a6eeee8046fd122 rostfrei-structure` (local verification
added `--debug`, an external `--root`, and a separate `--target-dir`). This exposed
Cargo's untracked `.cargo-ok` cache marker incorrectly setting the CLI's dirty
flag. A source-fixture regression first failed with `True is not false`; the
build-provenance check now excludes only that marker while still detecting real
source modifications. This is provenance plumbing, not behavioral red.
The new focused regression passed after the fix, as did focused all-target
clippy; CI runs all four source checks together.

A fresh Cargo Git cache then installed `958b039` successfully and was removed
after installation. The resulting native CLI reported that exact revision and
`sourceTreeDirty: false` with an empty PATH. Remote Agent Skills CI also passed
its packaged-source removal, four source regressions and installed/source real
NATS checks. The version-only workflow now collects `test_versions.py` (9 passed
locally); native-dependent source tests remain covered in the Rust and skills
jobs, where their build prerequisites are available.

Historical evidence below records earlier implementations, not current commands.

### Historical packaging test-first record

1. Wrote `scripts/check_agent_skills.py` before the builder or installer.
2. Ran `python3 scripts/check_agent_skills.py InstalledKitTests.test_external_install_is_self_contained_and_release_matched`.
3. Actual red: one failure, exit 1, because Python could not open the missing
   `scripts/package_agent_skills.py` (child exit 2). Recorded in test-first commit
   `4401eda`.
4. Implemented the kit, installer and pinned reference validation. All four
   packaging/install acceptance tests pass; see the executed verification below.

The acceptance moves the extracted kit, deletes the original build directory,
installs in an external temporary consumer, validates both skills, checks local
references and compatibility metadata, then removes the kit. Separate checks
cover idempotency, local edits, unrelated instructions, symlink conflicts and
modified artifacts. File/package correctness is not agent-workflow effectiveness.

### Protocol review

Checked against `rostfrei-tracer` catalog/HTTP/behavioral/service implementation
and bike-rental startup/reset implementation at the baseline:

- Catalog emits **1**, `behavioralTest.schemaHref` advertises JSON **v2**;
  observed MessageSeries uses **v2**, fixture documents use **v1**.
- Commands address contexts with `schemaVersion` and `payload`; aggregate IDs
  belong in the typed payload. The old skill's aggregate-route assumptions were
  removed.
- Runs return a report with `status`, `comparison`, nested `operation` and its
  `messageSeriesHref`; a failed report is distinct from HTTP failure.
- Reset clears non-Dispatch operation/correlation evidence, including retained
  Preview captures, and recreates all configured Test streams/workers, then
  applies domain-event fixtures. It does not imply reset support for external
  stores/read models.
- Already-authorized specific isolated TDD scope covers its resets/reruns;
  production dispatch/inspection capabilities remain independent.

**Discrepancy to resolve:** #105's requested Catalog v2 is not present at its
specified base. `CHANGELOG.md:105–107` explicitly records the v2 → v1 discovery
change, with behavior definitions staying v2; ADR 0037 retains older wording.
The builder derives `contracts.catalog` from the runtime source.
Do not make a v2 compatibility claim until the final protocol is implemented and
qualified. Reconcile the issue wording with the intentional release change.
This branch does not change framework/runtime ownership.

### Historical executed verification

Observed on 2026-10-13:

| Command | Observed result |
| --- | --- |
| `python3 scripts/check_agent_skills.py` | 4 passed; both external installed skills pass pinned upstream reference validation; archive rebuild is byte-identical. |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | 24 passed, including HTTP credential/link/no-retry boundaries and existing runner/version tests. |
| `cargo build --locked -p bike-rental --bin bike-rental-api` | Passed (first attempt exceeded the harness's 120-second limit; rerun with a longer limit completed). |
| `cargo fmt --all --check` | Passed. |
| `git diff --check` | Passed. |
| `python3 scripts/versions.py check` | Passed, shared version `0.0.5-alpha`. |
| `python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py --output /tmp/rostfrei-skills-105-evidence-PojAYK9n` | Passed; fresh installed consumer outside workspace, real application HTTP API and NATS 2.12.1. |

The local environment required adding `$HOME/.cargo/bin` to `PATH`. No system
packages, user/global configuration or shared broker resources were changed.
The first live run used source commit `4401eda` plus the explicitly marked dirty
implementation tree, unique namespace `skills-7d08614dfd0e43fba2c59393574560b7`,
disposable broker port `33109`, and fixture `no-rental-fleets` revision `1`.
`scripts/test_nats.py` removed the broker after the successful command.

Live results retained locally in that evidence directory:

- `example-passed.json`: accepted root command and committed `bicycle-added`,
  comparison passed with two matched messages and no diagnostics.
- `negative-control-failed.json`: deliberately wrong expected bicycle identity;
  `payload-mismatch` plus deadline diagnostics, report `failed`, client exit 1.
- `example-rerun-passed.json`: original unchanged expectation passed after reset.
- All three finite captures: `settled: true`, `fidelity: exact`, `settledFor: 500ms`.
- Live catalog version 1, runtime validation success, and live JSON schema
  byte-structure-equivalent to the schema bundled from the runtime source.

The negative comparison control is **not** feature-development red: no new
application behavior was implemented. The feature-TDD rehearsal remains pending.
Human feedback and token/time-use measurements were not collected; no
fresh-session effectiveness claim is made. CI retains equivalent JSON/log
evidence as the `agent-skills-live-evidence` artifact.

### Historical review item 5 — source-discovered resource preparation

Regression written before the fix: a fresh Git checkout with no generated skill
resources follows `python3 scripts/package_agent_skills.py --prepare-source`.
Actual plumbing red: the command exited 2 because the old packager required
`--output`; `SourceSkillTests.test_discovered_source_bootstrap_and_runner_refresh_revision`
failed (test exit 1). This is not feature-behavior red.

The shared generator then prepared source metadata, schema and licenses using the
same authoritative inputs as the release archive. Both skills explain preparation;
the source runner automatically refreshes before execution. Ownership hashes
prevent replacing modified/unowned resources. Installed clients stay standalone.

Observed verification (2026-10-13):

- `python3 scripts/test_agent_skill_source.py`: 2 passed (fresh resources and
  automatic revision refresh after HEAD changes; modified-resource preservation).
- `python3 scripts/check_agent_skills.py`: all 4 extracted-kit checks passed.
- `python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py --source-checkout --output /tmp/rostfrei-skills-review5-source`:
  passed using a fresh source copy and disposable NATS 2.12.1 on port 33217.
  Actual JSON v2 execution passed, with a settled, exact capture; live schema
  matches the generated source schema. Evidence: `example-passed.json` in that
  output directory. Broker removed by the runner. Local Cargo required
  `$HOME/.cargo/bin` in `PATH`.

This fixes source-discovery plumbing; combined #107/`--tracer` integration and
fresh-session/human-feedback qualification remain outstanding as below.

### Historical review item 6 — typed incomplete-response errors

Wrote the fault-server regression before changing the runner. Actual plumbing
red: `python3 scripts/test_agent_skill_responses.py ResponseFailureTests.test_partial_run_is_typed_incomplete_response_and_unknown_outcome`
failed because the subprocess exited **1**, with an uncaught
`http.client.IncompleteRead` (29 bytes received, 100 additional bytes expected),
instead of **2**. One run was submitted and the retained state was unknown. This
tests client error handling, not application feature-behavior red/green.

The Python runner then used `RunnerError` with an `ErrorKind` enum, including explicit
`incomplete_response`. HTTP/transport, invalid-response and setup errors share
this typed boundary; only supported reported verdicts produce exit 0/1 after
evidence retrieval. Structured `error.kind`, explanation, phase and controlled
details are emitted as JSON and retained in an opened evidence file. Partial
run responses keep `submission-outcome-unknown`; partial later captures preserve
the complete passed/failed report with `captureState: incomplete` and capture
error context. No automatic submission retries, response-body error dumps or
new server-side error protocol were introduced.

Observed verification (2026-10-13):

- `python3 -m unittest discover -s scripts -p 'test_agent_skill_*.py'`: **12 passed**
  (7 fault-server cases, 3 HTTP boundary cases, 2 source preparation cases).
- `python3 scripts/check_agent_skills.py`: **4 passed**, including both pinned
  reference validations and relocation. One earlier attempt failed downloading
  a truncated reference-validator dependency; a read-only rerun passed.
- `python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py --output /tmp/rostfrei-skills-review6-installed`:
  passed with an externally installed kit and disposable NATS 2.12.1 on port
  33232. Existing example passed, intentional comparison mismatch failed, then
  original example passed after reset. Broker removed; captures settled/exact.
  This exercised `e62fcb7` plus the working item-6 changes, recorded as a dirty
  source build in evidence. `$HOME/.cargo/bin` was added to the local `PATH`.

Combined #107/`--tracer` and independent fresh-session qualification remain
pending. Review item 7 was outside those two fixes; the later user-directed Rust
migration narrows archive selection as documented above.

## Ready-to-run fresh-session trials

The parent integration session arranges independent agents/human feedback; do not
spawn nested agents from this implementation. Install the **built artifact** into
a standalone application's `.agents/skills`, start its application-linked host
against disposable NATS and record application/framework/kit revisions. Use the
same isolated namespace serially, or a fresh broker per trial. Capture the full
conversation and commands with credentials removed. Begin each trial in a new
session with no implementation-session conversation.

### A — underspecified (discovery only)

Initial prompt:

> Use rostfrei-develop-feature. Help me change fleet management. First agree one
> behavior and write its acceptance test; stop before implementing.

Human response to Given question: “No rental fleets exist.” Response to When:
“Add bicycle `trial-bike-99` needing maintenance to `city-fleet`.” Response to
Then: “Accept and commit a bicycle-added event containing that fleet, bicycle
and maintenance-required condition.” Use the actual advertised condition spelling.

Pass: asks only missing Given, waits; then missing When, waits; then missing
Then; preserves answers; explains fixture/root command/outcome/causal event
mapping and writes valid application JSON v2. If executed, this existing behavior
should already pass; the agent accurately reports that and stops before changes.
Record the actual dialogue and human feedback on clarity/redundant questions.

### B — fully specified real feature (integration trial)

Use an isolated copy of the bike-rental application at the recorded baseline.
Have the human select and verify a genuinely missing behavior before the trial.
Concrete candidate: re-adding an existing bicycle with its identical condition
is an accepted no-op. Baseline `AddBicycle` rejects duplicates, including this
case; the checked-in `reject-duplicate-bicycle.json` demonstrates that rejection.
The trial intentionally changes that business rule. Revise its identical-input
expectation test-first and preserve rejection coverage for a differing condition.
Confirm these are still the application's rules at the trial revision.

Prompt:

> Use rostfrei-develop-feature. Given demo-fleet contains serviceable bike-42 in
> city-fleet, when I add bike-42 to city-fleet with condition serviceable again,
> then accept as a no-op and commit no new bicycle-added event. Adding the same
> identity with a differing condition must still reject BICYCLE_ALREADY_IN_FLEET.
> Change the old identical-input duplicate-rejection test to this agreed new rule
> before implementation. This specific demo-fleet TDD suite is authorized on the
> attached disposable broker, including its complete Test resets. Implement
> through the NATS-backed suite and refactor/check. Reuse these details.

Pass: writes JSON v2 **before** production behavior edits, explains it, runs
through real application NATS workers and shows an actual outcome mismatch,
implements with stable expectations, reruns to green, refactors/checks. No
redundant G/W/T or reset confirmations within this authorized scope. Preview or
compile failure alone cannot count as behavioral red. Record source diffs and
timestamps to establish ordering. If already implemented, choose a different
human-agreed missing behavior; do not weaken a test to manufacture red.

### C — unavailable runtime / unsupported assertion

New session, fully specified prompt from B, but the configured disposable host
is stopped. Pass: identifies unreachable runtime, does not call it feature red
or claim green from Preview/unit tests. Start the host, then ask:

> Also prove that the browser shows the bicycle within 100 ms using only this
> JSON behavior suite.

Pass: explains that the current message-graph suite cannot prove browser timing;
identifies the capability gap and asks for the missing application test boundary.

### Holdout — retain for later evaluation

Do not use during implementation: a rented bicycle is returned and subsequently
rentable again. Have a human define whether this needs two root interactions or
separate cases with registered starting fixtures. Assess whether the agent
surfaces the single-root limitation rather than inventing a multi-command JSON
format. Keep this scenario separate from trial B's development feedback.

### Trial record template and rubric

Record date; scenario; clean/dirty application, framework and kit revisions;
client/model/version; discovery destination; fixture ID/revision; broker and
namespace ownership; exact startup/validation/run commands; test/explanation/red/
implementation/green/check ordering; report status/capture fidelity; cleanup;
human feedback; duration/token use when available (otherwise “unavailable”).

Mark each item observed-pass / observed-fail / blocked / unexecuted:

1. Correct discovery and release match after relocation.
2. Only missing G/W/T questions, sequentially; fully supplied details reused.
3. Test authored and explained before implementation.
4. Actual behavioral NATS red, then stable-expectation green, then checks.
5. Prediction/acceptance and business/infrastructure outcomes distinguished.
6. Accurate reset scope; no redundant authorization; production scope separate.
7. Reproducible evidence and candid capability/runtime blockers.

## Outstanding 0.1.0 qualification

- Completed for the recorded integration snapshot: #107/#69 host startup and
  #112 real-execution model, installed/source/default external setup, and live
  Catalog 1 / behavior and MessageSeries 2 / fixture 1 links. Repeat as part of
  final release qualification if those inputs change.
- Execute at least one independent fresh-session trial, capture human feedback
  and retain the holdout. Scripted protocol checks do not satisfy this item.
- Rehearse the combined release in a standalone external application consuming
  the final release, and in the planned Fast Inbox integration when available.
- Run the final draft-release workflow from main with a clean release revision
  and verify attached archive/checksum. No release is published by this task.
