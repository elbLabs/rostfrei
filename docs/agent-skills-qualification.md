# Agent Skills qualification — issue #105

## Scope and evidence

Framework/application baseline: `1d7d86153d0dc96d461454a26902b14caf737d39`,
Rostfrei workspace version **0.0.5-alpha** (target milestone 0.1.0); bike-rental
application from that same revision. The kit follows the existing version source;
the eventual 0.1.0 release-version bump is separate release preparation.
Implementation branch: `feat/agent-skills-kit`. Agent/client: GPT-6 Astra in
OpenCode V2. This implementation session is not a fresh-session workflow trial.

### Packaging test-first record

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

### Executed verification

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

### Review item 5 — source-discovered resource preparation

Regression written before the fix: a fresh Git checkout with no generated skill
resources follows `python3 scripts/package_agent_skills.py --prepare-source`.
Actual plumbing red: the command exited 2 because the old packager required
`--output`; `SourceSkillTests.test_discovered_source_bootstrap_and_runner_refresh_revision`
failed (test exit 1). This is not feature-behavior red.

The shared generator now prepares source metadata, schema and licenses using the
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

- Integrate #69's actual standard application-linked host API and startup path;
  update both skill prerequisites and this guide, then rerun installed-kit checks
  plus the real-NATS example against it.
- Resolve Catalog version discrepancy with the final runtime; validate live
  discovery/schema/fixture/operation links against the release's protocol.
- Execute at least one independent fresh-session trial, capture human feedback
  and retain the holdout. Scripted protocol checks do not satisfy this item.
- Rehearse the combined release in a standalone external application consuming
  the final release, and in the planned Fast Inbox integration when available.
- Run the final draft-release workflow from main with a clean release revision
  and verify attached archive/checksum. No release is published by this task.
