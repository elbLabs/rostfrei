# Skills and shared Tracer host qualification

## Agent operating model

The service is protocol-independent; the implemented adapter is **HTTP JSON plus
SSE**. There is no MCP server. `cargo rostfrei test` is the native Rust suite
client. Installed skills teach direct Bash/curl for catalog discovery, recorded
operations, continuous observation, quarantine and causal explanations. Setup is
explicit and dependency-matched; starting the application never installs skills.

The primary path is real isolated Test/suite execution or read-only inspection of
recorded activity. Within an agreed isolated TDD loop, “run/test/try” executes for
real. “Show what happened” reads evidence. Ambiguous target/scope is clarified.
Legacy Preview submission is retired from the runtime, Studio and agent workflow.
Optional **Simulate handler** uses the same registered handler and core
`CommandExecutor::simulate` over configured history for explicit rule diagnostics.
Its distinct typed result contains a decision, starting versions and proposed,
uncommitted domain events, with no operation, receipt or observed causal trace.
Injected dependencies are not automatically sandboxed. Core `SimulationOutcome`
and focused handler tests remain supported. Historical simulation evidence remains
predicted/uncommitted; retired requests never silently publish to Test/Dispatch.

Observation is a partial rolling event window, not a completion verdict. The
agent discovers optional application/scope relations, follows list/detail hrefs,
uses the inspection capability from the first production catalog request, reads
coverage/retention/fidelity, and watches SSE only for a bounded requested period.
It preserves opaque generation strings and decimal-string revisions/counters.
Only explicit matching message/causation IDs establish graph edges.

## Explicit handler diagnostics refinement (2026-10-15)

The user-approved refinement retains useful **Simulate handler**, without
restoring legacy Preview's operation-shaped synthetic trace. Its optional catalog
relation is `contexts[].commands[].versions[].simulateHandlerHrefTemplate`;
follow that value with POST `{"schemaVersion":1,"payload":{...}}` using the
control token. The current route is
`/contexts/{context}/commands/{command}/simulate-handler`.

HTTP 200 returns a distinct `kind: "handlerSimulation"`, `committed: false`,
command/schema coordinates, typed `outcome.decision` (accepted/rejected), `streams`
with aggregate identities/base versions/read guards, and `proposedEvents` with
event type/schema/proposed versions and policy-controlled payloads. Rejection
discards pending events; accepted zero-event results are legitimate. There is no
operation ID, Location, receipt or observed MessageSeries. Legacy `/simulate`
remains 404. Rust callers use `Tracer::simulate_handler(..., CommandRequest)`;
there is no native CLI simulation subcommand.

Studio defaults to Test and deliberately selects **Simulate handler**, showing
an **Uncommitted · no acceptance receipt** result outside the observed graph.
The installed skills use it only for explicit simulation/rule-debugging questions,
never before every Test, as acceptance evidence, or as a fallback. It replays
configured history (Test history in the shared host), invokes the same real handler
and dependencies, and does not test broker delivery, atomic persistence/concurrency,
workers or downstream integrations. Injected dependencies are not automatically
sandboxed. Uncertain requests are not retried automatically.

| Qualification input | Exact revision |
| --- | --- |
| #106 canonical skills/capability helper | `dc1d83d714195037dec3662ecccc3a0931313662` |
| #107 unchanged shared host | `b4c79060e8238be695e13b45e0b6593e6d2c268c` |
| #112 handler/Studio implementation | `24c529146445210a856a7b1d54ec0da1949c9d29` |
| Clean detached combined implementation | `e4dd403082b0997fb300303c1a6e19767c4bae6b` |
| Fresh source fixture (matching CLI and host) | `811a1e91fc1bc3397bd3a7bfcfc04be43909c4c9` |

The current merge completed without conflicts, preserving the canonical `.agents`
source. No #107 history was merged into #106. The later #112 live Studio fixture
correction `697a458` uses the actual free-form identity fields and changes no
runtime behavior. Review #107 before #112, paired with #106.

Evidence root: `/tmp/rostfrei-harmony-4wflut29/handler-qualification/`. Input heads
are in `inputs.json`, the clean integration is retained in `combined.bundle`, and
`qualify.py` reproduces the matched package/native/live checks. Original absence
red was observed at `abc4380` (missing relation, HTTP 404, missing deliberate Studio
choice and absent capability after real external-host Test execution). The original
managed logs were unavailable when archiving; `red-observations.md` is explicitly
a session transcription. Earlier failed attempts are retained, not overwritten.

Confirmed controls:

- Clean **77-file Cargo package**, external native install and extracted build
  source removal; **7 kit/reference checks** passed (`package.log`,
  `native-install.log`, `kit-checks.log`).
- Installed and fresh-source paths each passed real-NATS **pass → intentional
  mismatch → unchanged pass** with complete, settled/exact captures. Each CLI and
  host matched the clean revision recorded above. Installed execution removed
  the extracted kit; both consumer PATHs excluded Cargo/Git/Python. Their handler
  diagnostic accepted a proposed bicycle
  from base version 1, rejected the existing bicycle with no proposals, and kept
  history at version 1 with unchanged observation (`installed/`, `source/`). Scoped catalog,
  quarantine, actual causation, finite/SSE observation and reset isolation passed.
- A standalone counter app pinned **all eleven resolved Rostfrei crates** to the
  combined revision. Default native `skills install` selected matching clean
  resources, and empty-PATH pass/mismatch/pass plus handler simulation passed.
  The handler proposed value 9 at base version 2 without appending. Unconfigured
  observation stayed undiscovered and returned `501 observation-unavailable`
  (`external-consumer-verified/`, reproducer `external-consumer.py`). The earlier
  attempt incorrectly assumed twelve dependencies; its log and consumer remain
  retained, and the corrected check verifies every actual resolved Git revision.
- #112's **154 Tracer tests**, **7 core/testing simulation controls**, **5 pinned
  external-host tests**, and **3 real external-observation tests** passed. The
  counter proposed value 9 from version 2 without changing persisted history or
  observation. Cancellation released reset/admission gates; Test retained separate
  operation capacity. Core/domain sources remain unchanged from #107.
- Studio lint/build, mocked interactions and **13 visual scenarios** passed;
  desktop accepted/rejected and mobile handler results were inspected. Five real
  HTTP suites and five live Studio suites passed, plus explicit live handler
  diagnostics with the real Test graph unchanged. Normal state survived Test resets
  (`studio-live-inputs.log`).

All owned hosts and disposable brokers stopped. These scripted controls are not
independent agent competence or human feedback.
The release/independent-trial gates below remain open.

## Historical integration inputs

Original #106 head: `a87199fc98ccb0d6ec5290e7d6553b38ec188712`.
Shared-host #107 head: `b4c79060e8238be695e13b45e0b6593e6d2c268c`.
Both source worktrees were clean. A separate detached local worktree merged the
two inputs without conflicts; neither PR branch received the other's history.

Initial combined revision: `25bf39479feb0ce1c08a77f24de355b57e705ca0`.
The original live driver actually failed with `Application startup exceeded 300
seconds`: it ran the Normal application without `--tracer`, so no catalog or
suite submission occurred. This is pipeline/startup red, not domain-behavior red.
The driver now explicitly enables Tracer, configures inspection, and runs both
installed and source pass → deliberate mismatch → unchanged pass controls.
Source mode builds its CLI and application host from the same fresh snapshot.

The focused HTTP/SSE acceptance helper is
`scripts/check_agent_tracer_capabilities.py`, called by the existing live driver.
It checks runtime responses instead of matching prose in Markdown. The existing
shared-host controls cover external publishers and Normal-state preservation.
The first live check corrected an assumption about generation formatting:
generations are opaque strings, not decimal counters.

## Earlier qualified real-execution snapshot (2026-10-15)

| Input | Exact revision |
| --- | --- |
| #106 skills/driver code | `8c97667d3de57de33d8bd82339fff48005706c81` |
| #107 shared host | `b4c79060e8238be695e13b45e0b6593e6d2c268c` |
| #112 runtime/Studio removal, based on #107 | `52b62041490b8d800a70a116b624ef3a851588f7` |
| Clean isolated combined snapshot | `2a5910782535a1eafb53f2ed3566fb9ba6275123` |
| Fresh source-checkout snapshot (matching CLI and host) | `ae197d61fd7a6453d4c3d4dd18960ff7ddff7e4b` |

#112 is the focused `feat/tracer-real-execution` follow-up, based on #107; #106
remains the independent `feat/agent-skills-kit` branch. Review/land #107 before
#112 and pair them with #106's skills. No remote merge or release was performed.
Combining #106 with #112 required one rename/delete resolution: retain #106's
authoritative `.agents/skills/rostfrei-tracer/SKILL.md`, remove the obsolete
`.opencode` copy. The detached integration is retained in `combined-real.bundle`.
Later documentation and the quarantine test's 403 → retired-route 404 correction
do not change the runtime/Studio/packaged-skill implementation tested here.

Observed functional green:

- Clean **77-file Cargo package**, external native installation and removal of
  package build source; **7 external kit/reference checks** passed.
- Installed and source paths each executed **pass → intentional mismatch →
  unchanged pass** through real NATS 2.12.15, with complete, settled/exact 500ms
  finite captures. Both used `no-rental-fleets` revision `1`; each host and CLI
  matched its recorded source revision. Consumer PATH excluded Cargo/Git/Python.
  The installed run removed the extracted kit before execution.
- Both discovered scoped catalogs, quarantine lists, Test/production observation
  snapshots and bounded initial SSE; inspected the actual Test event and its
  explicit causation; checked Test observation expiry after reset and independent
  production generation retention. Neither catalog advertised simulation.
- A standalone counter consumer outside the workspace pinned **all 12 Rostfrei
  crates** to the combined Git revision. Default native `skills install` (no
  kit/destination override) selected the same clean release. Its native runner
  repeated pass/mismatch/pass with empty PATH and exact/settled captures. Absent
  observation remained absent in discovery and returned `501 observation-unavailable`.
- #112's five pinned external-host tests passed against real NATS, including
  injected dependencies, repeated persisted Test behavior, retired-route 404 with
  unchanged history, reset/Normal isolation, readiness/failure and shutdown.
  The three shared-host observation tests passed with external Test/Normal events
  and reset/shutdown control. Core simulation/domain tests remained green.
- #112's Studio CI exercised all five real HTTP suites and Normal-state
  preservation, plus mocked interactions and desktop/mobile inspection. Local
  Studio lint/build, interactions and all ten visual scenes also passed.

Evidence root: `/tmp/rostfrei-harmony-4wflut29`. The final combined log is
`combined-real-final.log`; summaries and operation/capture evidence are in
`installed-real/`, `source-real/`, and `external-consumer-real-evidence/`.
Native installation is `cli-real/bin/cargo-rostfrei`; the retained external
reproducer is `external-consumer-real.py`. Brokers used ports **33296**, **33298**
and **33302** respectively and were removed by the wrappers; owned hosts stopped.
`real-execution-inputs.json` and `combined-real.bundle` retain provenance.

Earlier attempts are retained, not overwritten: startup without `--tracer`,
the opaque-generation and flattened-submission helper assumptions, and an initial
external consumer blocked by `cargo generate-lockfile --offline` before its local
Git source had been imported. The successful consumer performs that initial
checkout before locked/native execution. These were qualification defects, not
domain behavioral mismatches. The intended negative controls are actual comparison
failures followed by unchanged passes.

Final remote CI links are retained on [#106](https://github.com/elbLabs/rostfrei/pull/106)
and [#112](https://github.com/elbLabs/rostfrei/pull/112). This is scripted functional
qualification, not an independent agent-usability trial or human feedback.

## Reproduce functional qualification

In a fresh temporary detached worktree, combine the recorded skill/code and host
heads (or restore the retained combined Git bundle), then use that combined
revision for **both** the app dependencies and CLI. Do not install from an
unrelated global tool's bundle. Typical repository-maintenance commands:

```sh
cargo package --locked -p rostfrei-structure
# Extract target/package/rostfrei-structure-*.crate outside the checkout.
cargo install --locked --debug --path "$EXTRACTED_PACKAGE" --root "$EXTERNAL_CLI"
# Remove only that owned extraction directory before running the consumer checks.
export ROSTFREI_CLI="$EXTERNAL_CLI/bin/cargo-rostfrei"
python3 scripts/check_agent_skills.py
python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py --output "$NEW_EVIDENCE/installed"
python3 scripts/test_nats.py -- python3 scripts/check_agent_skills_live.py --source-checkout --output "$NEW_EVIDENCE/source"
python3 scripts/test_nats.py -- cargo test --locked -p bike-rental --test shared_host_observation -- --test-threads=1
python3 scripts/test_nats.py -- python3 scripts/check_bike_rental_host.py
```

The last two checks belong to the combined host tree. Individual #106 CI runs
the installed/source driver against its own application-linked host. That older
host accepts and ignores `--tracer`; the combined shared host requires it. An
explicit `--host-command` must include the flag itself and use the matched host.
Python here is repository-only orchestration. Native suite execution runs with
an empty tools PATH; rich HTTP exploration uses bounded curl with pipe-fed headers.

For the external consumer check, copy #107's `crates/rostfrei-tracer/tests/consumer`
outside the workspace, replace its three Rostfrei path dependencies with the
combined Git URL/exact revision, and generate its lockfile. Run the installed
native `cargo-rostfrei rostfrei skills install` there with **no kit/destination
override**. Verify every resolved Rostfrei crate and installed release metadata
matches the combined revision. Build/start that consumer on a disposable broker,
then run `tests/increment.json` with its installed `--release` metadata, a copied
definition expecting the wrong value, and the unchanged definition again. This
consumer deliberately has no observation feed: discovery must not invent one;
the unconfigured endpoint reports `501 observation-unavailable`.

## Independent installed-skill trial — still pending

A **new independent agent session** must receive a real domain task and follow
only the application's installed skills, not this implementation conversation or
the scripted expected outputs. Choose a new task with the human; do not count
replaying the worked JSON example as feature-development usability evidence.

Preparation for the teammate:

1. Supply an application checkout pinned to the qualified combined revision and
   its real host command. Explicitly install dependency-matched skills into its
   `.agents/skills`; retain the managed baseline. Start the new agent there so it
   discovers installed skills, not the framework's source skills.
2. Start a new disposable JetStream broker with loopback-only ports and no volume:
   `docker run --rm --detach --name <unique-owned-name> -p 127.0.0.1::4222 nats:2.12.15-alpine -js`.
   Read the assigned port with `docker port <unique-owned-name> 4222/tcp` and set
   `ROSTFREI_NATS_URL`. Give the app a unique `ROSTFREI_APPLICATION` too.
3. Generate distinct control/dispatch/inspection tokens into environment variables
   without echoing, tracing or saving them. Set the host's loopback
   `ROSTFREI_API_ADDR`, and the agent's matching `ROSTFREI_TRACER_URL` and scoped
   capability variables. Start the supplied host; the framework bike-rental
   command is `cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer`.
   Do not assume another app enables observation merely because this example does.
4. Agree once on the isolated TDD fixture/reset scope. Observe whether the agent
   discovers actual capabilities, executes real red/green, uses read-only evidence
   appropriately, recognizes partial/unavailable data, and explains only explicit
   causality. Record misunderstandings and human feedback, not just exit codes.
5. Stop the owned host and `docker stop <unique-owned-name>`, unset capability
   variables, and retain only non-secret evidence. Do not keep processes or tokens
   alive to hand off the trial.

The published starter still predates skills support. A skills-enabled published
starter pin, final external-consumer/Fast Inbox release rehearsal and draft-release
attachment verification remain separate release gates.
