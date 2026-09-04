import type {
  CorrelationEvent,
  MessageGraphNode,
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

export function expectedGraph(definition: TestDefinition): MessageGraphNode[] {
  const context = fixtureContext(definition, "idle")
  const command = definition.when.command
  const root: MessageGraphNode = {
    id: `preview-command-${definition.id}`,
    parentId: context.at(-1)?.id,
    hideIncomingEdge: true,
    kind: "command",
    name: command.name,
    schemaVersion: command.schemaVersion,
    payload: command.payload,
    aggregateType: command.aggregate.type,
    aggregateId: command.aggregate.id,
    status: "idle",
  }
  const expectations = definition.then.trace?.contains ?? []

  return [
    ...context,
    root,
    ...expectations.map<MessageGraphNode>((expectation, index) => ({
      id: `preview-event-${definition.id}-${index}`,
      parentId: root.id,
      edgeFidelity: "grouped",
      kind: expectation.kind,
      name: expectation.name,
      schemaVersion: expectation.schemaVersion,
      payload: expectation.payload,
      status: "idle",
    })),
  ]
}

export function correlationGraph(
  events: CorrelationEvent[],
  definition: TestDefinition,
  report: TestReport
): MessageGraphNode[] {
  const context = fixtureContext(definition, "accepted")
  const commandEvent = events.find((event) => event.type === "command")
  const resultEvent = events.find((event) => event.type === "command-result")
  const result =
    resultEvent?.type === "command-result" ? resultEvent.result : undefined
  const commandMessageId = getStringProperty(result, "commandMessageId")
  const rootId = `operation-${report.operationId}`
  const command = definition.when.command
  const root: MessageGraphNode = {
    id: rootId,
    parentId: context.at(-1)?.id,
    hideIncomingEdge: true,
    kind: "command",
    name:
      commandEvent?.type === "command" ? commandEvent.command : command.name,
    schemaVersion:
      commandEvent?.type === "command"
        ? commandEvent.schemaVersion
        : command.schemaVersion,
    payload: command.payload,
    response: result,
    messageId: commandMessageId,
    aggregateType:
      commandEvent?.type === "command"
        ? commandEvent.aggregateType
        : command.aggregate.type,
    aggregateId:
      commandEvent?.type === "command"
        ? commandEvent.aggregateId
        : command.aggregate.id,
    status:
      resultEvent?.type === "command-result"
        ? outcomeStatus(resultEvent.outcome)
        : "running",
  }

  const observedEvents = events.filter(
    (event) =>
      event.type === "domain-event" || event.type === "integration-event"
  )
  const knownIds = new Set(
    observedEvents.flatMap((event) =>
      event.messageId ? [event.messageId] : []
    )
  )

  return [
    ...context,
    root,
    ...observedEvents.map<MessageGraphNode>((event) => {
      const id = event.messageId ?? `correlation-event-${event.id}`
      const causedByCommand =
        event.causationId !== undefined &&
        event.causationId === commandMessageId
      const causedByMessage =
        event.causationId !== undefined && knownIds.has(event.causationId)
      const exactParent = causedByCommand || causedByMessage

      return {
        id,
        parentId: causedByCommand
          ? rootId
          : causedByMessage
            ? event.causationId
            : rootId,
        edgeFidelity: exactParent ? "exact" : "grouped",
        kind: event.type,
        name: event.eventType,
        schemaVersion: event.schemaVersion,
        payload: event.payload,
        messageId: event.messageId,
        causationId: event.causationId,
        status: "accepted",
      }
    }),
  ]
}

function fixtureContext(
  definition: TestDefinition,
  status: MessageGraphNode["status"]
): MessageGraphNode[] {
  const fixtureId = `fixture-${definition.id}`
  const context: MessageGraphNode[] = [
    {
      id: fixtureId,
      kind: "domain-event",
      name: definition.given.fixture,
      schemaVersion: definition.schemaVersion,
      payload: { fixture: definition.given.fixture },
      context: "fixture",
      status,
    },
  ]
  let parentId = fixtureId

  for (const [index, command] of (definition.given.commands ?? []).entries()) {
    const id = `setup-command-${definition.id}-${index}`
    context.push({
      id,
      parentId,
      edgeFidelity: "grouped",
      kind: "command",
      name: command.name,
      schemaVersion: command.schemaVersion,
      payload: command.payload,
      aggregateType: command.aggregate.type,
      aggregateId: command.aggregate.id,
      context: "setup",
      status,
    })
    parentId = id
  }

  return context
}

function outcomeStatus(
  outcome: TestReport["outcome"]
): MessageGraphNode["status"] {
  switch (outcome) {
    case "accepted":
      return "accepted"
    case "rejected":
      return "rejected"
    case "failed":
    case "indeterminate":
      return "failed"
    default:
      return "idle"
  }
}

function getStringProperty(
  value: unknown,
  property: string
): string | undefined {
  if (typeof value !== "object" || value === null) return undefined
  const candidate = Reflect.get(value, property)
  return typeof candidate === "string" ? candidate : undefined
}
