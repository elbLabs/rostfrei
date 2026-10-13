# Release and execution prerequisites

For repository-discovered source skills, first run
`python3 scripts/package_agent_skills.py --prepare-source` from the framework root
(Git and Python 3.11+). Repeat after checkout/input changes before using metadata.
This generates the relative resources from the packager's authoritative inputs
and preserves modified files. Installed skills need no source preparation.

An installed skill includes `references/release.json` relative to its root. It
records the framework release/source revision, whether the build tree was dirty,
and independent Catalog, behavior-definition, MessageSeries and fixture formats.
Check the application's dependency revision as well as the live catalog.

At the issue #105 base (`1d7d861`), `catalogVersion` is **1**, behavior definitions
and observed MessageSeries are **2**, and registered fixtures are **1**. The issue
requested Catalog v2, but `CHANGELOG.md` explicitly records discovery reverting
from v2 to v1 while behavior definitions stay v2. ADR 0037 predates that change.
Do not call Catalog v1 v2 or confuse it with command payload schema versions.
Reconcile the issue wording with this recorded release change and verify #69's
final host/protocol before the 0.1.0 rehearsal. Stop on an unsupported catalog.

Commands address bounded contexts. For example, `add-bicycle` schema 2 contains
`fleet_id`, `bicycle_id` and `condition` in its JSON payload. The typed HTTP
submission wraps it as `{"schemaVersion":2,"payload":{...}}`. No aggregate ID is
added to the command route. Use actual advertised relations and schema versions.

## Start a real isolated runtime

The consumer must supply its real handlers, supervised workers, fixtures and
NATS event store with JetStream on NATS Server 2.12.1 or newer. Use its documented
binary and dependencies. A Tracer endpoint without a Test/reset relation does
not provide behavior acceptance. Preview is not a substitute.

For the matching framework's current bike-rental example, set distinct control
and dispatch tokens in the environment, then from the framework checkout run:

```sh
python3 scripts/test_nats.py -- cargo run --locked -p bike-rental --bin bike-rental-api
```

The Python 3.11+/Docker runner starts a disposable pinned JetStream broker and
cleans it up when the application exits. Rust is required to build the host.
`ROSTFREI_API_ADDR` defaults to `127.0.0.1:1309`. Configure
`ROSTFREI_TRACER_URL` to this origin for client requests. The current example
requires both `ROSTFREI_API_TOKEN` and `ROSTFREI_DISPATCH_TOKEN` at startup and
provisions both traffic scopes; client Test requests use only the control token.
There is no generic `cargo rostfrei dev` in this baseline. #69's final
application-linked startup path remains an explicit integration handoff.

Do not reset unknown shared NATS resources. A disposable broker or uniquely
owned `ROSTFREI_APPLICATION` isolates the current example. A correlation/run ID
does not isolate Test resources, and separate Test runs must be serialized until
#68 supplies independent isolation. Reset covers the host's complete Test
topology, not just a named fixture or aggregate. External stores/read models are
only covered by explicit application reset support. Production capabilities
remain separate even when a TDD loop has authorized repeated isolated runs.
