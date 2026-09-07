export type MessageKind = "command" | "domain-event" | "integration-event"

export type MessageStatus =
  | "idle"
  | "running"
  | "accepted"
  | "rejected"
  | "failed"

export type EdgeFidelity = "exact" | "grouped"
export type EdgeRelationship = "causation" | "stream-order" | "context"

export interface MessageGraphNode {
  id: string
  parentId?: string
  hideIncomingEdge?: boolean
  edgeFidelity?: EdgeFidelity
  edgeRelationship?: EdgeRelationship
  context?: "fixture"
  kind: MessageKind
  name: string
  schemaVersion: number
  payload?: unknown
  response?: unknown
  messageId?: string
  causationId?: string
  aggregateType?: string
  aggregateId?: string
  streamVersion?: number
  status?: MessageStatus
}

export interface AggregateReference {
  type: string
  id: string
}

export interface FixtureDomainEvent {
  kind: "domain-event"
  messageId: string
  correlationId: string
  causationId?: string
  name: string
  schemaVersion: number
  aggregate: AggregateReference
  streamVersion: number
  payload: unknown
}

export interface Fixture {
  schemaVersion: number
  id: string
  revision: string
  messages: FixtureDomainEvent[]
}

export interface TestDefinitionSummary {
  id: string
  name: string
  revision: string
  definitionHref: string
  runHref: string
}

export type ExpectedOutcome =
  | "accepted"
  | { rejected: { code: string; payload?: unknown } }

interface ExpectedMessageNodeBase {
  key: string
  parentKey?: string
  name: string
  schemaVersion: number
  payload?: unknown
}

export type ExpectedMessageNode =
  | (ExpectedMessageNodeBase & {
      kind: "command"
      aggregate: AggregateReference
      outcome: ExpectedOutcome
    })
  | (ExpectedMessageNodeBase & {
      kind: "domain-event" | "integration-event"
    })

export interface MessageGraphDefinition {
  within?: string
  settleFor?: string
  nodes: ExpectedMessageNode[]
}

export interface MessageSeriesDefinition {
  within: string
  settleFor: string
  graphs: MessageGraphDefinition[]
}

export interface TestDefinition {
  schemaVersion: number
  id: string
  name: string
  setup: {
    fixture: string
  }
  expected: MessageSeriesDefinition
}

export interface TestDefinitionRevision {
  revision: string
  definition: TestDefinition
}

interface ObservedMessageBase {
  messageId: string
  correlationId: string
  causationId?: string
  observationOrder: number
  name: string
  schemaVersion: number
  aggregate?: AggregateReference
  payload?: unknown
}

export type ObservedMessage =
  | (ObservedMessageBase & {
      kind: "command"
      aggregate: AggregateReference
    })
  | (ObservedMessageBase & {
      kind: "domain-event"
    })
  | (ObservedMessageBase & {
      kind: "integration-event"
    })

export type CommandResponseOutcome =
  | { status: "accepted"; value: null }
  | {
      status: "rejected"
      value: {
        classification: string
        code: string
        message: string
        details?: unknown
      }
    }

export interface ObservedCommandOutcome {
  responseMessageId: string
  commandMessageId: string
  correlationId: string
  observationOrder: number
  outcome: CommandResponseOutcome
}

export interface ObservedMessageSeries {
  messages: ObservedMessage[]
  commandOutcomes: ObservedCommandOutcome[]
}

export interface MessageSeriesComparison {
  status: "passed" | "failed"
  matches: Array<{
    expectedKey: string
    observedMessageId: string
  }>
  diagnostics: Array<{
    code: string
    path: string
    message: string
    expected?: unknown
    observed?: unknown
  }>
}

export interface OperationSnapshot {
  operationId: string
  correlationId: string
  operationEventsHref: string
  correlationEventsHref: string
  messageSeriesHref: string
  events: {
    kind: "predicted" | "observed"
    href: string
  }
  mode: "simulate" | "test" | "dispatch"
  status: "queued" | "running" | "completed" | "failed" | "indeterminate"
  command: string
  schemaVersion: number
  aggregateType: string
  aggregateId: string
  latestEventId: number
  result?: unknown
  failure?: {
    code: string
    message: string
    commandMessageId?: string
    duplicate?: boolean
  }
}

export interface TestReport {
  runId: string
  testId: string
  revision?: string
  status: "passed" | "failed"
  expected: MessageSeriesDefinition
  observed: ObservedMessageSeries
  comparison: MessageSeriesComparison
  commandOutcome?: ObservedCommandOutcome
  operationId: string
  correlationId: string
  operationHref: string
  operationEventsHref: string
  correlationEventsHref: string
  operation: OperationSnapshot
}

export interface StoredRun {
  runId: string
  testId: string
  testName: string
  status: TestReport["status"]
  createdAt: string
  nodes: MessageGraphNode[]
}
