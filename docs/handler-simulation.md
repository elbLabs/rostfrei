# Handler simulation

**Simulate handler** evaluates the registered handler against configured history
and returns its decision, starting versions, and proposed events. It persists no
events or outcome and creates no operation or observed trace. Use real Test for
behavioral acceptance. Injected dependencies are not automatically sandboxed.

## Interface

Discover `contexts[].commands[].versions[].simulateHandlerHrefTemplate` in the
control catalog. Follow that link with the control bearer token:

```text
POST /contexts/{context}/commands/{command}/simulate-handler
Content-Type: application/json

{"schemaVersion":1,"payload":{...}}
```

No idempotency key is needed. Rust callers use
`Tracer::simulate_handler(context, command, CommandRequest)`.

HTTP 200 returns `HandlerSimulationResult` with `kind: "handlerSimulation"`,
`committed: false`, the accepted/rejected decision, loaded stream base versions,
and proposed events. Rejection has no proposed events; acceptance may also be empty.
The result has no receipt, operation/correlation ID, capture, or Location header.
Default payload policy removes event payloads and redacts rejection/failure details.
Oversized results return `413 handler-result-too-large` rather than partial success.

## Scope and limits

The shared host uses isolated Test history. `with_test_event_store` selects that
history for custom builders; otherwise the supplied `EventHistory` defines scope.
Tracer-mediated reset gates diagnostics and failed reset makes them unavailable.
A direct failed `NatsRuntime::reset` does not update Tracer's separate diagnostic
health flag; readable history alone does not prove writable authority is healthy.

The diagnostic concurrency pool is separate from Test/Dispatch admission. A
30-second deadline or caller cancellation aborts unfinished async work, but cannot
undo external dependency effects or interrupt arbitrary synchronous handler code.
Do not automatically retry. Payload, history, codec, panic, and timeout failures
are typed errors, not business rejections.

Studio keeps this result in the Command panel, separate from observed graphs and
behavioral verdicts. The retired Preview `/simulate` route returns 404; use catalog
links rather than assuming diagnostic availability.
