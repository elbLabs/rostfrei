import type {
  EdgeRelationship,
  Fixture,
  MessageGraphNode,
  ObservedCommandOutcome,
  OperationMessageSeries,
  OperationSnapshot,
  TestDefinition,
  TestReport,
} from "@/lib/types"

export interface PositionedNode extends MessageGraphNode {
  x: number
  y: number
}

export interface GraphEdge {
  id: string
  source: PositionedNode
  target: PositionedNode
  fidelity: "exact" | "grouped"
  relationship: EdgeRelationship
  context: boolean
}

export interface GraphLayout {
  nodes: PositionedNode[]
  edges: GraphEdge[]
}

const HORIZONTAL_GAP = 280
const LANE_GAP = 140

export function layoutMessageGraph(nodes: MessageGraphNode[]): GraphLayout {
  const byId = new Map(nodes.map((node) => [node.id, node]))
  const positioned = new Map<
    string,
    PositionedNode & { depth: number; lane: number }
  >()
  const occupiedByDepth = new Map<number, Set<number>>()
  const childCount = new Map<string, number>()
  const pending = [...nodes]
  let rootCount = 0

  const place = (node: MessageGraphNode) => {
    const parent = node.parentId ? positioned.get(node.parentId) : undefined
    const depth = parent ? parent.depth + 1 : 0
    const occupied = occupiedByDepth.get(depth) ?? new Set<number>()
    let preferredLane: number

    if (parent) {
      const siblingIndex = childCount.get(parent.id) ?? 0
      childCount.set(parent.id, siblingIndex + 1)
      preferredLane = parent.lane + siblingLaneOffset(siblingIndex)
    } else {
      preferredLane = rootLane(rootCount)
      rootCount += 1
    }

    const lane = nearestFreeLane(preferredLane, occupied)
    occupied.add(lane)
    occupiedByDepth.set(depth, occupied)
    positioned.set(node.id, {
      ...node,
      depth,
      lane,
      x: depth * HORIZONTAL_GAP,
      y: lane * LANE_GAP,
    })
  }

  while (pending.length > 0) {
    let placedInPass = false
    for (let index = 0; index < pending.length;) {
      const node = pending[index]
      const parentIsReady =
        !node.parentId ||
        !byId.has(node.parentId) ||
        positioned.has(node.parentId)
      if (!parentIsReady) {
        index += 1
        continue
      }
      place(node)
      pending.splice(index, 1)
      placedInPass = true
    }
    if (!placedInPass) place(pending.shift()!)
  }

  const positionedNodes = nodes.map((node) => positioned.get(node.id)!)
  const positionedById = new Map(positionedNodes.map((node) => [node.id, node]))
  const edges = positionedNodes.flatMap((target) => {
    if (!target.parentId || target.hideIncomingEdge) return []
    const source = positionedById.get(target.parentId)
    if (!source) return []
    return [
      {
        id: `${source.id}:${target.id}`,
        source,
        target,
        fidelity: target.edgeFidelity ?? "grouped",
        relationship: target.edgeRelationship ?? "causation",
        context: Boolean(source.context || target.context),
      },
    ]
  })

  return {
    nodes: positionedNodes,
    edges,
  }
}

function siblingLaneOffset(index: number): number {
  if (index === 0) return 0
  const distance = Math.ceil(index / 2)
  return index % 2 === 1 ? distance : -distance
}

function rootLane(index: number): number {
  if (index === 0) return 0
  const distance = Math.ceil(index / 2) * 2
  return index % 2 === 1 ? distance : -distance
}

function nearestFreeLane(preferred: number, occupied: Set<number>): number {
  if (!occupied.has(preferred)) return preferred
  for (let distance = 1; distance <= occupied.size + 1; distance += 1) {
    if (!occupied.has(preferred + distance)) return preferred + distance
    if (!occupied.has(preferred - distance)) return preferred - distance
  }
  return preferred + occupied.size + 1
}

export function expectedGraph(
  definition: TestDefinition,
  fixture?: Fixture
): MessageGraphNode[] {
  const context = fixtureContext("idle", fixture)
  const expectedNodes = definition.expected.graphs[0]?.nodes ?? []
  const idsByKey = new Map(
    expectedNodes.map((node) => [
      node.key,
      `expected-${definition.id}-${node.key}`,
    ])
  )

  return [
    ...context.nodes,
    ...expectedNodes.map<MessageGraphNode>((node) => {
      const expectedParentId = node.parentKey
        ? idsByKey.get(node.parentKey)
        : undefined
      const isRoot = !node.parentKey
      return {
        id: idsByKey.get(node.key)!,
        parentId: isRoot ? context.anchorId : expectedParentId,
        edgeFidelity: expectedParentId ? "exact" : undefined,
        edgeRelationship: isRoot && context.anchorId ? "context" : undefined,
        kind: node.kind,
        name: node.name,
        schemaVersion: node.schemaVersion,
        payload: node.payload,
        boundedContext: node.kind === "command" ? node.context : undefined,
        status: "idle",
      }
    }),
  ]
}

export function reportGraph(
  report: TestReport,
  fixture?: Fixture
): MessageGraphNode[] {
  const context = fixtureContext("accepted", fixture)
  const messages = [...report.observed.messages].sort(
    (left, right) => left.observationOrder - right.observationOrder
  )
  const knownIds = new Set(messages.map((message) => message.messageId))
  const expectedRoot = report.expected.graphs[0]?.nodes.find(
    (node) => !node.parentKey && node.kind === "command"
  )
  const matchedRootId = report.comparison.matches.find(
    (match) => match.expectedKey === expectedRoot?.key
  )?.observedMessageId
  const subject =
    messages.find((message) => message.messageId === matchedRootId) ??
    messages.find((message) => message.kind === "command")
  const outcomes = new Map(
    report.observed.commandOutcomes.map((outcome) => [
      outcome.commandMessageId,
      outcome,
    ])
  )
  if (report.commandOutcome) {
    outcomes.set(report.commandOutcome.commandMessageId, report.commandOutcome)
  }

  return [
    ...context.nodes,
    ...messages.map<MessageGraphNode>((message) => {
      const exactParent = message.causationId
        ? knownIds.has(message.causationId)
        : false
      const isSubject = message.messageId === subject?.messageId
      const fallbackParent =
        !isSubject && subject ? subject.messageId : context.anchorId
      const outcome = outcomes.get(message.messageId)

      return {
        id: message.messageId,
        parentId: exactParent ? message.causationId : fallbackParent,
        edgeFidelity: exactParent ? "exact" : "grouped",
        edgeRelationship: isSubject && context.anchorId ? "context" : undefined,
        kind: message.kind,
        name: message.name,
        schemaVersion: message.schemaVersion,
        payload: message.payload,
        response:
          message.kind === "command"
            ? commandResponse(outcome, report, isSubject)
            : undefined,
        messageId: message.messageId,
        causationId: message.causationId,
        boundedContext:
          message.kind === "command" ? message.context : undefined,
        aggregateType:
          message.kind === "domain-event" ? message.aggregate?.type : undefined,
        aggregateId:
          message.kind === "domain-event" ? message.aggregate?.id : undefined,
        status:
          message.kind === "command"
            ? commandStatus(outcome, report, isSubject)
            : "accepted",
      }
    }),
  ]
}

export function operationGraph(
  operation: OperationSnapshot,
  series: OperationMessageSeries
): MessageGraphNode[] {
  const messages = [...series.messageSeries.messages].sort(
    (left, right) => left.observationOrder - right.observationOrder
  )
  const knownIds = new Set(messages.map((message) => message.messageId))
  const subject = messages.find((message) => message.kind === "command")
  const outcomes = new Map(
    series.messageSeries.commandOutcomes.map((outcome) => [
      outcome.commandMessageId,
      outcome,
    ])
  )

  return messages.map<MessageGraphNode>((message) => {
    const exactParent = message.causationId
      ? knownIds.has(message.causationId)
      : false
    const isSubject = message.messageId === subject?.messageId
    const outcome = outcomes.get(message.messageId)

    return {
      id: message.messageId,
      parentId: exactParent ? message.causationId : undefined,
      edgeFidelity: exactParent ? "exact" : "grouped",
      kind: message.kind,
      name: message.name,
      schemaVersion: message.schemaVersion,
      payload: message.payload,
      response:
        message.kind === "command"
          ? (outcome?.outcome ??
            (isSubject ? (operation.result ?? operation.failure) : undefined))
          : undefined,
      messageId: message.messageId,
      causationId: message.causationId,
      boundedContext: message.kind === "command" ? message.context : undefined,
      aggregateType:
        message.kind === "domain-event" ? message.aggregate?.type : undefined,
      aggregateId:
        message.kind === "domain-event" ? message.aggregate?.id : undefined,
      status:
        message.kind === "command"
          ? operationCommandStatus(outcome, operation, isSubject)
          : "accepted",
    }
  })
}

function fixtureContext(
  status: MessageGraphNode["status"],
  fixture?: Fixture
): { nodes: MessageGraphNode[]; anchorId?: string } {
  const streams = new Map<string, Fixture["messages"]>()
  for (const message of fixture?.messages ?? []) {
    const streamKey = `${message.aggregate.type}\u0000${message.aggregate.id}`
    const stream = streams.get(streamKey) ?? []
    stream.push(message)
    streams.set(streamKey, stream)
  }

  const context: MessageGraphNode[] = []
  let anchorId: string | undefined
  let anchorDepth = 0
  for (const [streamIndex, stream] of [...streams.values()].entries()) {
    stream.sort((left, right) => left.streamVersion - right.streamVersion)
    let parentId: string | undefined
    for (const message of stream) {
      const id = `fixture-event-${streamIndex}-${message.messageId}`
      context.push({
        id,
        parentId,
        edgeFidelity: "exact",
        edgeRelationship: parentId ? "stream-order" : undefined,
        kind: "domain-event",
        name: message.name,
        schemaVersion: message.schemaVersion,
        payload: message.payload,
        messageId: message.messageId,
        causationId: message.causationId,
        aggregateType: message.aggregate.type,
        aggregateId: message.aggregate.id,
        streamVersion: message.streamVersion,
        context: "fixture",
        status,
      })
      parentId = id
    }
    if (parentId && stream.length > anchorDepth) {
      anchorId = parentId
      anchorDepth = stream.length
    }
  }

  return { nodes: context, anchorId }
}

function commandStatus(
  outcome: ObservedCommandOutcome | undefined,
  report: TestReport,
  isSubject: boolean
): MessageGraphNode["status"] {
  if (outcome?.outcome.status === "accepted") return "accepted"
  if (outcome?.outcome.status === "rejected") return "rejected"
  if (!isSubject) return "accepted"
  if (report.operation.status === "failed") return "failed"
  if (report.operation.status === "indeterminate") return "indeterminate"
  return report.operation.status === "completed" ? "indeterminate" : "running"
}

function operationCommandStatus(
  outcome: ObservedCommandOutcome | undefined,
  operation: OperationSnapshot,
  isSubject: boolean
): MessageGraphNode["status"] {
  if (outcome?.outcome.status === "accepted") return "accepted"
  if (outcome?.outcome.status === "rejected") return "rejected"
  if (!isSubject) return "idle"
  if (operation.status === "failed") return "failed"
  if (operation.status === "indeterminate") return "indeterminate"
  const decision = decisionOf(operation.result)
  if (decision === "accepted" || decision === "rejected") return decision
  return operation.status === "completed" ? "indeterminate" : "running"
}

function decisionOf(result: unknown): "accepted" | "rejected" | undefined {
  if (typeof result !== "object" || result === null) return undefined
  const decision = Reflect.get(result, "decision")
  return decision === "accepted" || decision === "rejected"
    ? decision
    : undefined
}

function commandResponse(
  outcome: ObservedCommandOutcome | undefined,
  report: TestReport,
  isSubject: boolean
): unknown {
  if (outcome) return outcome.outcome
  if (!isSubject) return undefined
  return report.operation.result ?? report.operation.failure
}
