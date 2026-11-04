import type { HistoricalMessage, MessageGraphNode } from "@/lib/types"

export function historicalGraph(
  items: HistoricalMessage[],
  aggregate: boolean,
  timeline = false
): MessageGraphNode[] {
  const responses = new Map<string, HistoricalMessage>()
  if (timeline) {
    const requests = new Map<string, HistoricalMessage[]>()
    const outcomes = new Map<string, HistoricalMessage[]>()
    for (const item of items) {
      const map =
        item.evidence === "command"
          ? requests
          : item.evidence === "command-response"
            ? outcomes
            : undefined
      if (map)
        map.set(commandKey(item), [...(map.get(commandKey(item)) ?? []), item])
    }
    for (const [key, candidates] of requests) {
      const outcome = outcomes.get(key)
      // Merge only one unambiguous request followed by one response. Retried or
      // conflicting evidence stays separate; a response never invents a request.
      if (
        candidates.length === 1 &&
        outcome?.length === 1 &&
        items.indexOf(candidates[0]) < items.indexOf(outcome[0])
      )
        responses.set(candidates[0].id, outcome[0])
    }
    const merged = new Set([...responses.values()].map((item) => item.id))
    items = items.filter((item) => !merged.has(item.id))
  }
  // A retained response and a still-queued request can identify the same command.
  // Keep their evidence separate unless one unambiguous parent identity exists.
  const byMessage = new Map<string, HistoricalMessage[]>()
  for (const item of items)
    byMessage.set(item.messageId, [
      ...(byMessage.get(item.messageId) ?? []),
      item,
    ])
  const previous = new Map<string, HistoricalMessage>()
  return items.map((item) => {
    const response = responses.get(item.id)
    const parents = item.causationId
      ? byMessage.get(item.causationId)
      : undefined
    const parent =
      parents?.length === 1 && parents[0].correlationId === item.correlationId
        ? parents[0]
        : undefined
    const stream = JSON.stringify([item.aggregateType, item.aggregateId])
    const predecessor =
      aggregate && item.kind === "domain-event"
        ? previous.get(stream)
        : undefined
    previous.set(stream, item)
    return {
      id: item.id,
      kind: item.kind,
      name: item.name,
      schemaVersion: item.schemaVersion,
      payload: item.payload,
      response: response?.response ?? item.response,
      responseStoredAt: response?.storedAt,
      messageId: item.messageId,
      causationId: item.causationId,
      correlationId: item.correlationId,
      boundedContext: item.context,
      aggregateType: item.aggregateType,
      aggregateId: item.aggregateId,
      streamVersion: item.streamVersion,
      storedAt: item.storedAt,
      evidence: item.evidence,
      parentId: parent?.id ?? predecessor?.id,
      edgeRelationship: parent
        ? "causation"
        : predecessor
          ? "stream-order"
          : undefined,
      edgeFidelity: parent ? "exact" : item.causationId ? "grouped" : undefined,
      subject: false,
      status:
        item.kind === "command"
          ? (response?.response ?? item.response)?.status
          : "accepted",
    }
  })
}

function commandKey(item: HistoricalMessage): string {
  return JSON.stringify([
    item.context,
    item.name,
    item.correlationId,
    item.messageId,
  ])
}
