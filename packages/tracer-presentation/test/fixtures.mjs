// Synthetic presentation fixtures, not evidence of a runtime execution. Business
// names/payloads come from the registered example; IDs mirror Studio's fixtures.
import definition from '../../../examples/bike-rental/tests/tracer/rent-available-bicycle.json' with { type: 'json' }
import fixture from '../../../examples/bike-rental/fixtures/demo-fleet.json' with { type: 'json' }

export function savedEvidence() {
  const nodes = definition.expected.graphs[0].nodes
  const ids = ['command-rent', 'event-rented', 'integration-started']
  const messages = nodes.map((node, index) => ({
    kind: node.kind,
    messageId: ids[index],
    correlationId: 'correlation-1',
    ...(index ? { causationId: ids[index - 1] } : { context: 'bike-rental' }),
    observationOrder: index,
    name: node.name,
    schemaVersion: node.schemaVersion,
    payload: node.payload,
  }))
  const observed = {
    messages,
    commandOutcomes: [{
      commandMessageId: ids[0], responseMessageId: 'response-1',
      correlationId: 'correlation-1', observationOrder: 3,
      outcome: { status: 'accepted' },
    }],
  }
  const operation = {
    operationId: 'operation-1', correlationId: 'correlation-1', mode: 'test',
    status: 'completed', context: 'bike-rental', command: 'rent-bicycle',
    schemaVersion: 2, latestEventId: 5,
    operationEventsHref: '/operations/operation-1/events',
    correlationEventsHref: '/correlations/correlation-1/events',
    messageSeriesHref: '/operations/operation-1/message-series',
    events: { kind: 'observed', href: '/correlations/correlation-1/events' },
    result: {
      decision: 'accepted', published: true,
      commandMessageId: ids[0], responseMessageId: 'response-1', duplicate: false,
    },
  }
  return {
    release: { version: '0.1.3', revision: 'presentation-fixture' },
    cliRelease: { version: '0.1.3', revision: 'presentation-fixture' },
    catalogVersion: 1, validation: { valid: true }, schema: {}, definition, fixture,
    runState: 'reported', captureState: 'complete',
    report: {
      runId: 'run-1', testId: definition.id, status: 'passed',
      expected: definition.expected, observed,
      comparison: { status: 'passed', matches: [], diagnostics: [] },
      operationId: operation.operationId, correlationId: operation.correlationId,
      operationHref: '/operations/operation-1',
      operationEventsHref: operation.operationEventsHref,
      correlationEventsHref: operation.correlationEventsHref, operation,
    },
    messageSeries: {
      operationId: operation.operationId, correlationId: operation.correlationId,
      mode: 'test', messageSeries: structuredClone(observed),
      capture: { settled: true, settledFor: '500ms', fidelity: 'exact' },
    },
  }
}

export function simulation() {
  return {
    kind: 'handlerSimulation', committed: false, context: 'bike-rental',
    command: 'rent-bicycle', schemaVersion: 2, outcome: { decision: 'accepted' },
    streams: [{
      aggregateType: 'RentalFleet', aggregateId: 'city-fleet',
      baseStreamVersion: '18446744073709551614', readGuard: false,
    }],
    proposedEvents: [{
      aggregateType: 'RentalFleet', aggregateId: 'city-fleet',
      proposedStreamVersion: '18446744073709551615',
      eventType: 'bicycle-rented', schemaVersion: 1, payload: { bicycle_id: 'bike-42' },
    }],
  }
}
