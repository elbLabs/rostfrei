# Rostfrei Studio

A focused React interface for discovering Rostfrei Tracer commands,
publishing isolated Test commands, dispatching protected production commands,
and inspecting their durable responses. Read-only Simulate is available as a
secondary preview. Studio does not define domain
commands, aggregate instances, fields, choices, modes, or reset actions; it
loads them from authenticated Tracer APIs.

Studio follows the catalog's action links, streams the correlated command,
committed domain events, public integration events, and command result over a
resumable SSE endpoint, and polls the returned operation resource for terminal
status. The correlation stream stays open after that status so asynchronous
effects can continue to arrive. Simulate keeps its predicted events visibly
separate from Test and Dispatch effects observed through the application pipeline.

## Development

Start the bike-rental Tracer service from the repository root:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  ROSTFREI_API_TOKEN=local-development-token \
  ROSTFREI_DISPATCH_TOKEN=local-dispatch-token \
  cargo run --locked -p bike-rental
```

Then start Studio:

```sh
cd studio
bun install
bun run dev
```

Vite proxies Tracer API paths to `http://127.0.0.1:1309` and attaches the appropriate local
capability server-side, so Studio connects automatically without
exposing it to browser code. Discovery, Simulate, Test, and reset use
`ROSTFREI_API_TOKEN`; Dispatch and its operation and correlation resources use
`ROSTFREI_DISPATCH_TOKEN`. The local defaults are `local-development-token` and
`local-dispatch-token`. For remote deployments, use the Connection panel and the
operator-provided control and dispatch capabilities, subject to CORS policy.

## Bike-rental walkthrough

1. Leave the default `Test` environment selected and run `Rent bicycle` for
   `city-fleet` and `bike-42`. The result is accepted with command publication
   and durable response evidence. Observe resulting event effects through the
   test application pipeline.
2. Run the same command again. The application returns a durable rejection from
   its updated test state.
3. Use `Reset test data` and run it once more to confirm the deterministic test
   environment is restored.
4. Select `Dispatch` and run `Rent bicycle` for `bike-99` to exercise the
   separately authorized production transport: the application rejects that
   maintenance-required bicycle.

Test and Dispatch publish commands through separately authorized transports and
wait for durable application responses. Aggregate discovery and runtime input
choices currently come from test history, so Studio allows manual aggregate IDs
and JSON payloads; production always validates the command against its own
current state.

## Checks

```sh
bun run typecheck
bun run build
```
