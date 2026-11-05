# Explore rostfrei with an agent

Ask a coding agent to prepare a local bike-rental demo, run a successful rental
and a business rejection, then explain the real Tracer evidence one step at a
time. The default experience is text in your agent conversation.

Copy this prompt into an agent with terminal and file access:

> Help me explore rostfrei through its bike-rental example. Use https://github.com/elbLabs/rostfrei and read docs/agent-tutorial.md from the checkout before setup. Check my environment, prepare an isolated local demo, and guide me through a successful rental and a rejection using real Tracer evidence. Explain one step at a time and let me choose what to inspect next. Use text by default; use a richer display only when its adapter is explicitly available.

This guide bootstraps the source-checkout experience. Select a checkout
containing this guide; older installed kits do not gain references when online
docs change.
For an existing application's release-matched skills, see
[Agent Skills setup](agent-skills-install.md).

## Instructions for the agent

Read the selected checkout's `AGENTS.md`, [documentation index](README.md),
[Tracer skill](../.agents/skills/rostfrei-tracer/SKILL.md), its
[runtime prerequisites](../.agents/skills/rostfrei-tracer/references/runtime.md)
and **[guided tutorial reference](../.agents/skills/rostfrei-tracer/references/guided-tutorial.md)**.
Follow that reference for the teaching loop and presentation-adapter contract.
These instructions extend the existing Tracer skill; no third skill is needed.

Explain the immediate setup step, carry it out, and report its result briefly.
Once the demo is ready, pause at each tutorial checkpoint and let the learner
choose what to inspect or when to continue. Use ordinary conversation if the
harness has no dedicated question tool.

### 1. Select and check the checkout

If a checkout already exists, inspect its location, Git remote, selected
branch/tag and full commit, and local changes. Preserve local work and use that
checkout's docs, code and toolchain together. Do not silently switch revisions.
If none exists, clone into a new owned directory:

```sh
git clone https://github.com/elbLabs/rostfrei.git
cd rostfrei
git rev-parse HEAD
git status --short
```

Honor a requested release/ref and check that it contains this guide and the
linked tutorial reference. If it does not, explain the mismatch before choosing
a tutorial-capable revision. Record the exact commit; a moving branch name is
not release provenance. A live catalog reports contracts, not the host's source
revision, so build the host and source CLI from this same checkout.

### 2. Check only the prerequisites this path needs

Inspect what is already available before installing anything:

- **Git, Rust/Cargo and a native linker.** Read `rust-toolchain.toml` and use its
  exact pin, currently Rust **1.98.0**. From the repository root, check
  `rustup show active-toolchain`, `rustc --version` and `cargo --version`; resolve
  overrides that would bypass the pin. Use `--locked` for builds.
- **Bash and curl** for the skill's safe HTTP discovery examples. The Rust CLI
  handles behavior execution; Python is not a tutorial prerequisite.
- **NATS Server 2.12.1+ with JetStream**, either an owned local server or Docker
  with Compose for the supplied example configuration. Check server/engine
  availability, free loopback ports and writable disposable storage.
- **Network access** if Git sources, the pinned toolchain, Cargo dependencies or
  a broker image still need fetching.

Explain a missing prerequisite and install only what the selected setup needs,
using the platform's available package manager when necessary. Node, pnpm,
Studio and a browser are not required for text. Check frontend/package-manager
requirements only if a separately available presentation adapter needs them
and the learner chooses it. MCP availability alone does not imply a visual app.

Check the source CLI using the repository alias:

```sh
cargo rostfrei-dev skills info
```

Use `cargo rostfrei-dev ...` throughout this source tutorial, not a possibly
unrelated globally installed CLI. Source skills need no generated `release.json`
or install step; the source CLI supplies build provenance. An installed skill's
`references/release.json` instead belongs to its resolved application release.

### 3. Prepare an owned local runtime

Read [bike-rental startup](../examples/bike-rental/README.md#run-locally) and its
broker configuration. Prefer a disposable NATS instance with fresh storage.
For an already running broker, establish a unique application namespace owned
by this demo before starting any workers. Never reuse the sample namespace from
a previous tutorial or reset unknown shared resources.

Set a fresh `ROSTFREI_APPLICATION` and an explicit, new
`ROSTFREI_COMPLETED_OUTCOME_EPOCH`. The epoch selects fresh writable authority;
it must not reinterpret an existing history. Keep these values fixed during
this demo, including both behavior runs. The host provisions Normal and Test
resources and resets Test on startup. Each behavior run resets the entire Test
scenario again; the tutorial reference explains preserving evidence between runs.

For Docker Compose, use a new project name and its owned volume. The supplied
file binds loopback port 4222, so check that it is free first:

```sh
docker compose -p "$ROSTFREI_APPLICATION" -f examples/bike-rental/compose.yaml up -d
```

For a native NATS server, use an owned temporary configuration/storage directory,
loopback bind and an available port. Preserve the example configuration's
`max_payload` requirement and enable JetStream. Verify readiness and version
before launching the application. Set `ROSTFREI_NATS_URL` to that exact broker.

Generate distinct `ROSTFREI_API_TOKEN` and `ROSTFREI_DISPATCH_TOKEN` in the process
environment using an available secure random source. The API host needs both;
tutorial execution and inspection use the control token. Keep tokens out of
output, argv, files and evidence, following the Tracer skill's HTTP rules.
Arrange for the host and client processes to inherit the needed environment.

Use the example's bounded local stream limits:

```sh
export ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES=67108864
export ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES=268435456
export ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES=524288
export ROSTFREI_API_ADDR=127.0.0.1:1309
export ROSTFREI_TRACER_URL=http://127.0.0.1:1309
cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer
```

If 1309 is occupied, choose a free loopback port and change both API address and
client URL together. Keep the host running in a managed background process or
separate terminal and retain its handle for cleanup. `--bin bike-rental-api`
selects the actual application; `--tracer` enables its HTTP capability. This
host uses the application's real handlers, workers, fixtures and NATS store.

### 4. Discover, run, inspect, pause

Use the skill's bounded, authenticated `/catalog` request and advertised links
to confirm the host, schema contracts, registered definitions and fixtures.
Prepare a new writable `EVIDENCE_DIR` outside the checkout to hold both saved
JSON results. Follow the [guided tutorial reference](../.agents/skills/rostfrei-tracer/references/guided-tutorial.md):

1. Read `rent-available-bicycle`, `reject-unavailable-bicycle` and their
   registered `demo-fleet` fixture; explain Given/When/Then.
2. Run the successful rental of serviceable `bike-42`. Read its saved report,
   operation and finite MessageSeries, explain the observed result, and pause.
3. Let the learner inspect that evidence. Verify it is saved before the next
   run resets live Test evidence.
4. When the learner continues, run the rejection of maintenance-required
   `bike-99` from a fresh `demo-fleet`. Explain the explicit rejected outcome
   separately from the absence of events, and pause again.

Render existing JSON as text through the agent. Source rules can explain the
domain, but do not invent policy logs or query projections in the trace. Richer
presentation uses only an explicitly available adapter and the same evidence.
Read-only questions or display changes never trigger another test.

#### Optional saved-evidence readers

The private [presentation package](../packages/tracer-presentation/README.md)
offers deterministic text and message lookup for adapters in this checkout.
It reads the saved CLI JSON without connecting to Tracer. It is optional;
ordinary agent conversation still reads the same evidence without Node or a UI.

Keep the complete saved report and finite capture as the source of truth.
Graph nodes, summaries and conversation context are derived views. A selection
belongs to one immutable capture plus its existing operation and message IDs;
operation IDs alone can repeat across separate runtimes. Adapters must validate
the capture binding before restoring a selection or explaining a message.

Readers distinguish suite verdict, command decision and capture quality. They
retain missing or redacted payloads and incomplete evidence as limitations, and
link messages only through explicit matching causation and message IDs. Switching
displays reuses that evidence and selection. Rendering has no publication, reset,
simulation or credential capability.

MCP connectivity does not establish an embedded UI. Qualify show, inspect and
selection feedback for the particular host and surface. An unavailable graphical
adapter falls back to text over the same saved capture.

The [source-checkout probes](../experiments/studio-adapters/README.md) document
opt-in host experiments and their prerequisites.

### 5. Finish and clean up

Summarize the two observed outcomes and give the learner both evidence paths.
Stop owned application/broker processes when the learner finishes. For the
Compose project created above, remove only that project's disposable storage:

```sh
docker compose -p "$ROSTFREI_APPLICATION" -f examples/bike-rental/compose.yaml down -v
```

For a reused broker, keep cleanup limited to the demo's owned namespace and
report any resources left there. Retain saved evidence for later inspection.
