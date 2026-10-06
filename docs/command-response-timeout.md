# Bounded command-response waiting

`NatsMessagingAdapter::new` waits up to **30 seconds** for a command response by
default (`DEFAULT_COMMAND_RESPONSE_TIMEOUT`). Applications with longer-running
commands can set their own budget:

```rust
use std::time::Duration;
use rostfrei::nats::{NatsCommandResponseReader, NatsMessagingAdapter, NatsPublisher};

fn adapter(publisher: NatsPublisher, reader: NatsCommandResponseReader) -> NatsMessagingAdapter {
    NatsMessagingAdapter::new(publisher, reader)
        .with_response_timeout(Duration::from_secs(60))
}
```

The response budget starts **when response reading begins**, after command
publication, the publication observer and any replay-authority admission. It
bounds response-reader retries, including absent responses and temporary read
failures. Authorization, publication and its retries, and observer work are outside
this budget. A host that needs an end-to-end request deadline must set that
separately.

With `with_replay_publication()`, an authorized completed command is published and
its current durable winner is checked again before response reading. A usable
winner returns immediately even if the raw response has expired; the response-wait
budget is used only when reading a response is still necessary.

## Meaning of a timeout

The caller receives `CommandBusErrorKind::Timeout`; the standard HTTP adapter maps
it to HTTP 504 with code `rostfrei.http.command-timeout`. This means **no final
outcome was confirmed to this caller**. It does not imply non-commit, cancel the
queued command, create a business rejection, or automatically republish it.

A worker can finish after the caller stops waiting. Recovery must preserve the
original operation ID, command identity and canonical command evidence. An
authorized completed-outcome lookup can return that original acceptance or
rejection without executing the handler again, subject to current authorization
and detail-expiry policy. See the [completed-outcome contract](command-runtime.md#identity-and-results).

## Quarantine and integration events

Some command deliveries are quarantined without producing a response, for example
when trusted caller-context resolution refuses a delivery. The sender now stops
waiting at its configured deadline. Quarantine remains inspection/recovery
evidence; its existence does not prove that a command never committed. Repair and
command redrive remain explicit, identity-preserving actions under the
[technical-failure recovery procedure](command-failures.md).

Ordinary integration-event publication waits for the broker's publication receipt,
not subscriber completion, and uses its existing publish timeout. An integration
subscriber that dispatches a command through this adapter uses the same finite
response budget. Its existing retry/quarantine policy still applies; retry must
retain the derived command identity. A legitimate command rejection remains a
completed outcome, not an integration-delivery failure.
