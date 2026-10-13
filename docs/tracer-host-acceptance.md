# Automatic NATS host acceptance (#69)

The first acceptance is `crates/rostfrei-tracer/tests/consumer`: a standalone
counter application with an injected handler dependency, a MessageSeries fixture,
and a JSON v2 behavior definition. It registers the command/handler once. The
check copies the consumer outside this workspace and exercises real HTTP and
JetStream, checking persisted history as well as the behavior report.

Run with:

```sh
python3 scripts/test_nats.py -- python3 scripts/check_tracer_host.py
```

The repository runner creates and removes its own NATS 2.12.1 container. It never
uses the default broker. A first local attempt failed before compilation because
`/tmp/opencode` was not writable; the system temporary-directory fallback is used
for the external qualification.

Red (2026-10-13): isolated NATS 2.12.1 ready; external consumer compilation
failed only with unresolved host/binding APIs (`E0432`, `E0433`, `E0425`). This
is a missing-API red, not an observed business-behavior failure.

Protocol check: the current catalog is **v1**, fixture documents are **v1**, and
behavior definitions are **v2**. The checked-in skill's aggregate-in-command
instructions predate bounded-context commands; code and advertised links govern.
