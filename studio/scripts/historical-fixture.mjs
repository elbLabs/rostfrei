export const historicalCapability = {
  application: "visual-fixture",
  scope: "test",
  messagesHref: "/historical/test/messages",
  aggregateTypes: ["bike-rental/rental-fleet"],
}

const base = {
  context: "bike-rental",
  correlationId: "rental-history",
  storedAt: "2026-10-01T10:00:00Z",
}
export const historicalMessages = [
  {
    ...base,
    id: "domain.1",
    messageId: "rented",
    kind: "domain-event",
    evidence: "domain-event",
    name: "bicycle-rented",
    schemaVersion: 1,
    causationId: "rent-command",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    streamVersion: "9007199254740993",
    payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
  },
  {
    ...base,
    id: "response.1",
    messageId: "rent-command",
    kind: "command",
    evidence: "command-response",
    name: "rent-bicycle",
    response: { status: "accepted" },
  },
  {
    ...base,
    id: "integration.1",
    messageId: "published",
    kind: "integration-event",
    evidence: "integration-event",
    name: "bicycle-rental-started",
    schemaVersion: 1,
    causationId: "rented",
    payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
  },
]

export function historicalPage(items = historicalMessages, nextHref) {
  return {
    application: "visual-fixture",
    scope: "test",
    items,
    scannedMessages: items.length,
    skippedMessages: 0,
    partial: true,
    ...(nextHref ? { nextHref } : {}),
  }
}
