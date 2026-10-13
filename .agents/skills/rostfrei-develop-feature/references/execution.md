# Execute against real NATS

Prerequisites: the application's Rust toolchain and actual application-linked
Tracer, NATS Server **2.12.1+ with JetStream**, registered fixtures, supervised
Test workers, NATS event store, and the control capability. Read the installed
`references/release.json` relative to the skill root. The application/framework
revision must match the kit; the HTTP catalog alone does not identify a release.

Use a disposable broker or a uniquely owned application namespace. Record which
one. Never reset an unknown shared `127.0.0.1:4222` environment. Test-vs-Normal
separation does not isolate concurrent Test runs (#68). Run a suite serially.

Each behavior run resets **all configured Test resources**, including event
history/receipts, messaging/quarantine streams and consumers, and invalidates
retained Test and Preview operations/captures. It reapplies the registered
fixture's domain events. Normal resources are separate. External stores/read models require
application-specific reset support. Establish this scope once for the requested
TDD loop and reuse that authorization for its specific suite/fixtures.

## Supported startup today

Use the consuming application's documented application-linked binary; the kit
does not supply application handlers or a generic development host. In the
matching Rostfrei source release, the implemented example binary is:

```sh
python3 scripts/test_nats.py -- cargo run --locked -p bike-rental --bin bike-rental-api
```

Run this from the **framework checkout**, with Rust and Docker available. The
runner starts a pinned disposable JetStream broker, sets `ROSTFREI_NATS_URL` and
bounded resource limits, and removes the broker after the child exits. Set
distinct `ROSTFREI_API_TOKEN` and `ROSTFREI_DISPATCH_TOKEN` in the host environment
(the current example requires both); do not display or persist them.
`ROSTFREI_API_ADDR` defaults to `127.0.0.1:1309` and `ROSTFREI_APPLICATION`
defaults to `bike-rental`. This example provisions both Normal and Test topology,
and resets Test on startup. Never use it against an unowned shared namespace.

For an external application, locate its equivalent binary and fixtures rather
than copying example handlers. The standard application-linked host/startup API
from #69 and the final 0.1.0 installed-kit rehearsal are pending integration.
There is no supported `cargo rostfrei dev` command in this baseline.

## Installed-kit command

Set `ROSTFREI_TRACER_URL` to the isolated host's origin and make its control token
available as `ROSTFREI_API_TOKEN` in the agent environment. Run from the consumer
directory, replacing `SKILL` with the installed skill directory:

```sh
SKILL="$PWD/.agents/skills/rostfrei-develop-feature"
python3 "$SKILL/scripts/run_behavior.py" \
  "$SKILL/assets/add-first-bicycle.json" --output behavior-evidence.json
```

Python 3.11+ is sufficient; no framework checkout, pip, host-specific question
tool, or dispatch credential is needed by this client. The server must already
have the fixture registered. The command validates the exact JSON, checks
release-matched catalog discovery, reads the fixture, runs once using the
advertised link, and captures the report and finite operation message series.
It uses bounded timeouts, disables proxies/redirects and never retries a POST.
Exit **0** means the report passed; **1** means the report failed (inspect whether
the diagnostics show the agreed behavioral mismatch); **2** means the run was
blocked or evidence incomplete. Existing evidence files are preserved.

Do not print tokens, put them in argv/URLs or write them to files. Evidence can
contain application payloads; keep it local or redact before sharing. An
ambiguous request can have executed: inspect existing operations before deciding
what to do next. `status: failed` in an HTTP 200 report is still a failed suite.

Report `capture.settled`, `fidelity`, expected/observed differences and the actual
business decision. A missing event is not proven by an unsettled partial capture.
Mermaid edges require matching explicit `causationId`/`messageId`; ordering alone
does not prove causality. Preview provides predictions and never acceptance.
