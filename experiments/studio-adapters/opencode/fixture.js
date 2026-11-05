// Deliberately synthetic, abbreviated evidence. Never a real Tracer test result.
function freeze(value) {
  if (value && typeof value === "object") {
    Object.values(value).forEach(freeze)
    Object.freeze(value)
  }
  return value
}

export const fixture = freeze({
  source: "synthetic adapter fixture; no Tracer execution",
  runState: "reported",
  captureState: "complete",
  report: {
    status: "passed",
    operationId: "synthetic-operation-1",
    correlationId: "synthetic-correlation-1",
    operation: {
      operationId: "synthetic-operation-1", correlationId: "synthetic-correlation-1",
      mode: "test", status: "completed", context: "bike-rental", command: "rent-bicycle",
      schemaVersion: 2, latestEventId: "5",
      result: { decision: "accepted", published: true, commandMessageId: "synthetic-command-1", responseMessageId: "synthetic-response-1" },
    },
  },
  messageSeries: {
    operationId: "synthetic-operation-1",
    correlationId: "synthetic-correlation-1",
    mode: "test",
    capture: { settled: false, settledFor: "1ms", fidelity: "grouped", note: "Synthetic, deliberately incomplete capture" },
    messageSeries: {
      messages: [
        { messageId: "synthetic-command-1", correlationId: "synthetic-correlation-1", observationOrder: 0, kind: "command", context: "bike-rental", name: "rent-bicycle", schemaVersion: 2, payload: { bicycle_id: "synthetic-bike" } },
        { messageId: "synthetic-event-1", correlationId: "synthetic-correlation-1", observationOrder: 1, causationId: "synthetic-command-1", kind: "domain-event", name: "bicycle-rented", schemaVersion: 1, payload: { marker: "explicit-reveal-only" } },
        { messageId: "synthetic-orphan-1", correlationId: "synthetic-correlation-1", observationOrder: 2, causationId: "synthetic-missing-parent", kind: "integration-event", name: "bicycle-rental-started", schemaVersion: 1 },
      ],
      commandOutcomes: [{ commandMessageId: "synthetic-command-1", responseMessageId: "synthetic-response-1", correlationId: "synthetic-correlation-1", observationOrder: 3, outcome: { status: "accepted", value: null } }],
    },
  },
})
