export type MessageKind = "command" | "domain-event" | "integration-event"

export type MessageStatus =
  "idle" | "running" | "accepted" | "rejected" | "failed"

export type EdgeFidelity = "exact" | "grouped"

export interface MessageGraphNode {
  id: string
  parentId?: string
  hideIncomingEdge?: boolean
  edgeFidelity?: EdgeFidelity
  context?: "fixture" | "setup"
  kind: MessageKind
  name: string
  schemaVersion: number
  payload?: unknown
  response?: unknown
  messageId?: string
  causationId?: string
  aggregateType?: string
  aggregateId?: string
  status?: MessageStatus
}

export interface TestDefinitionSummary {
  id: string
  name: string
  revision: string
  runHref: string
}

export interface TestCommand {
  name: string
  schemaVersion: number
  aggregate: {
    type: string
    id: string
  }
  payload: unknown
}

export interface TraceExpectation {
  kind: "domain-event" | "integration-event"
  name: string
  schemaVersion: number
  payload?: unknown
}

export interface TestDefinition {
  schemaVersion: number
  id: string
  name: string
  given: {
    fixture: string
    commands?: TestCommand[]
  }
  when: {
    command: TestCommand
  }
  then: {
    outcome: "accepted" | { rejected: { code: string; payload?: unknown } }
    within: string
    trace?: {
      contains?: TraceExpectation[]
    }
  }
}

export interface TestDefinitionRevision {
  revision: string
  definition: TestDefinition
}

export interface TestReport {
  runId: string
  testId: string
  revision: string
  status: "passed" | "failed"
  operationId: string
  correlationId: string
  outcome?: "accepted" | "rejected" | "failed" | "indeterminate"
  failure?: {
    code: string
    message: string
  }
}

interface CorrelationEventBase {
  id: number
  correlationId: string
}

export type CorrelationEvent =
  | (CorrelationEventBase & {
      type: "command"
      operationId: string
      command: string
      schemaVersion: number
      aggregateType: string
      aggregateId: string
    })
  | (CorrelationEventBase & {
      type: "domain-event"
      eventType: string
      schemaVersion: number
      messageId?: string
      causationId?: string
      streamVersion?: number
      payload?: unknown
    })
  | (CorrelationEventBase & {
      type: "integration-event"
      eventType: string
      schemaVersion: number
      messageId?: string
      causationId?: string
      subject?: string
      payload?: unknown
    })
  | (CorrelationEventBase & {
      type: "command-result"
      operationId: string
      outcome: "accepted" | "rejected" | "failed" | "indeterminate"
      result?: unknown
    })

export interface StoredRun {
  runId: string
  testId: string
  testName: string
  status: TestReport["status"]
  outcome?: TestReport["outcome"]
  createdAt: string
  nodes: MessageGraphNode[]
}
