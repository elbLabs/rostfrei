import {
  snapshotCapability,
  snapshotReconstruction,
} from "./snapshot-fixture.mjs"

export const timelineCapability = {
  ...snapshotCapability,
  timelineHref: "/historical/test/messages?order=time",
}
const base = {
  context: "bike-rental",
  correlationId: "rental-timeline",
  storedAt: "2026-10-01T10:00:00Z",
}
// Two adjacent nanoseconds and an exact timestamp tie exercise lossless ordering.
export const timelineMessages = [
  {
    id: "request.1",
    messageId: "rent",
    kind: "command",
    evidence: "command",
    name: "rent-bicycle",
    schemaVersion: 1,
    payload: { bicycle_id: "bike-42" },
  },
  {
    id: "domain.1",
    messageId: "rented",
    kind: "domain-event",
    evidence: "domain-event",
    name: "bicycle-rented",
    causationId: "rent",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    streamVersion: "1",
    payload: { bicycle_id: "bike-42" },
  },
  {
    id: "response.1",
    messageId: "rent",
    kind: "command",
    evidence: "command-response",
    name: "rent-bicycle",
    response: { status: "accepted" },
  },
  {
    id: "integration.1",
    messageId: "published",
    kind: "integration-event",
    evidence: "integration-event",
    name: "rental-started",
    causationId: "rented",
    payload: { bicycle_id: "bike-42" },
  },
  {
    id: "response.2",
    messageId: "reject",
    kind: "command",
    evidence: "command-response",
    name: "rent-bicycle",
    response: { status: "rejected", rejection: { message: "Already rented" } },
  },
  {
    id: "domain.2",
    messageId: "returned",
    kind: "domain-event",
    evidence: "domain-event",
    name: "bicycle-returned",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    streamVersion: "2",
    payload: { bicycle_id: "bike-42" },
  },
].map((item, index) => ({
  ...base,
  ...item,
  storedAtUnixNanos: String(
    1790848800000000000n +
      [0n, 1n, 1000000000n, 1000000000n, 3000000000n, 5000000000n][index]
  ),
}))

export function timelinePage(items = timelineMessages, nextHref) {
  return {
    application: timelineCapability.application,
    scope: "test",
    generation: "snapshot-history",
    order: "time",
    items,
    scannedMessages: items.length,
    skippedMessages: 0,
    nextHref,
    partial: true,
  }
}

export function timelineReconstruction(params) {
  return snapshotReconstruction(
    params.get("useSnapshots") !== "false",
    params.get("version") || "2"
  )
}
