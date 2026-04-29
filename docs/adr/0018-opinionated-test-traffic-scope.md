# ADR 0018: Test traffic is a derived application scope

## Status

Accepted.

## Decision

An application has one canonical `ApplicationName`. Deployment labels such as
`prod` are not part of that identity. Normal traffic uses the canonical,
unsuffixed application namespace. Rostfrei derives an optional test traffic
scope from the same application:

```text
Normal: <application>.<kind>.<bounded-context>.<name>
Test:   <application>.test.<kind>.<bounded-context>.<name>
```

`TrafficScope::Normal` is the default for existing constructors.
`TrafficScope::Test` is selected once when constructing a test bounded context;
addresses, command responses, queries, quarantine subjects, consumers, and
durables inherit it. Only the literal `test` is accepted as the optional second
subject token.

Quarantine and private domain-event subjects follow the same rule:

```text
<application>.quarantine.<source-kind>.<bounded-context>.<name>
<application>.test.quarantine.<source-kind>.<bounded-context>.<name>

<application>.domain.<bounded-context>.aggregate.<digest>
<application>.test.domain.<bounded-context>.aggregate.<digest>
```

Normal and test traffic use separate JetStream resources. Test stream names use
an unambiguous `__TEST` boundary so they cannot collide with the normal resources
of an application whose name ends in `-test`:

```text
<APPLICATION>_COMMANDS
<APPLICATION>__TEST_COMMANDS

<APPLICATION>_COMMAND_RESPONSES
<APPLICATION>__TEST_COMMAND_RESPONSES

<APPLICATION>_INTEGRATION_EVENTS
<APPLICATION>__TEST_INTEGRATION_EVENTS

<APPLICATION>_QUARANTINE
<APPLICATION>__TEST_QUARANTINE

<APPLICATION>__<CONTEXT>_DOMAIN_EVENTS
<APPLICATION>__TEST__<CONTEXT>_DOMAIN_EVENTS
```

Test consumer and durable names similarly insert `--test` after the canonical
application name. Publishers, readers, consumers, query adapters, event stores,
and correlation observers validate both application identity and traffic scope.
Normal components reject test addresses and test components reject normal
addresses.

The application recorded in stored NATS event envelopes remains the canonical
application. Traffic scope is represented by the validated subject and stream
identity rather than by inventing a second logical application.

Test reset is destructive and is permitted only for a test-scoped runtime.
Normal resources fail closed if passed to the reset capability.

This decision extends ADR 0015 and supersedes ADR 0011's statement that Test and
Dispatch differ by application scope. They share one application identity and
differ by traffic scope and resource lifecycle.

## Consequences

Applications provide one stable name. For bike-rental that name is
`bike-rental`; `bike-rental-prod` and `bike-rental-test` are no longer runtime
identities.

Subject isolation and stream isolation are both required. A separate stream
alone cannot safely distinguish Test from normal publishers and subscribers,
and overlapping JetStream filters may be rejected. Core NATS query traffic has
no stream boundary, so its scoped subject is the complete isolation mechanism.

The broad permission `<application>.>` includes both scopes. Deployments that
require credential isolation must grant the normal kind prefixes separately
from `<application>.test.>`.

Changing an existing deployment from `<application>-prod` to the canonical
application changes subjects, stream names, and stored event-envelope scope.
Existing authoritative histories require an explicit export and re-append
migration; they must not be copied into the new stream unchanged. Resettable old
test resources can be discarded and recreated. Before the normal-traffic
cutover, operators must either drain queued commands and integration events or
explicitly accept their loss. Retained command responses and durable consumer
positions do not migrate automatically; preserve their retry and delivery
semantics with an explicit migration, or retire them only after the associated
work has completed.
