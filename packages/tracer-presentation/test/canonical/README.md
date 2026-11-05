# Native conformance evidence

These files are byte-for-byte copies of real saved CLI evidence from bike-rental's
NATS-backed Test runtime, captured on 2026-11-05. They are not synthetic runs or
new executions by the presentation tests. No values, missing fields, identities,
payloads or provenance were edited. The files contain no capability credentials.

| Fixture | Registered behavior | SHA-256 |
| --- | --- | --- |
| `accepted.json` | `rent-available-bicycle` | `d99e755cd5f92bc3741685d04a89aba9db26de3deb2eab3d219533418dff38fb` |
| `rejected.json` | `reject-unavailable-bicycle` | `3eaab73f65802130c3c80ff1cd42a98cb474832edae901391aad700e46778339` |
| `earlier-accepted.json` | `rent-available-bicycle`, separate execution | `e0faf34da8c68c192428771c02147e388b1663437f411e5d10aaa9d51728f6e3` |

Each retains its own release revision, dirty-tree marker and source fingerprint.
The pair came from an isolated Test runtime with a fresh NATS container;
the accepted capture was retained before the rejection reset Test state.
The earlier capture is a distinct execution with recurring operation and event
identities, and is deliberately retained to test stale-snapshot selection.

Tests verify these hashes and use separately labelled in-memory mutations for
adversarial cases. Those mutations are not claims about runtime behavior.

See the [tutorial](../../../../docs/agent-tutorial.md) to produce new evidence.
