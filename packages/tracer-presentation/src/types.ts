// Read-only projections of the current Rust contracts. Unknown source fields
// remain in Presentation.source; this is not a replacement wire schema.
declare const rawNumber: unique symbol
/** Native JSON.rawJSON numeric token: local-only, not a structured-clone DTO. */
export interface RawJsonNumber { readonly rawJSON: string; readonly [rawNumber]: true }
export type Json = null | boolean | number | string | readonly Json[] | JsonObject | RawJsonNumber
export interface JsonObject { readonly [key: string]: Json }

export interface Message {
  readonly kind: 'command' | 'domain-event' | 'integration-event'
  readonly messageId: string
  readonly correlationId: string
  readonly causationId?: string | null
  readonly observationOrder: number
  readonly name: string
  readonly schemaVersion: number
  readonly context?: string
  readonly aggregate?: { readonly type: string; readonly id: string } | null
  readonly payload?: Json
}

export interface CommandOutcome {
  readonly commandMessageId: string
  readonly responseMessageId: string
  readonly correlationId: string
  readonly observationOrder: number
  readonly outcome:
    | { readonly status: 'accepted'; readonly value?: null }
    | { readonly status: 'rejected'; readonly value: JsonObject }
}

export interface Operation {
  readonly operationId: string
  readonly correlationId: string
  readonly mode: 'test' | 'dispatch'
  readonly status: 'queued' | 'running' | 'completed' | 'failed' | 'indeterminate'
  readonly context: string
  readonly command: string
  readonly schemaVersion: number
  readonly latestEventId: string
  readonly result?: {
    readonly decision: 'accepted' | 'rejected'
    readonly published: true
    readonly commandMessageId?: string
    readonly responseMessageId?: string
    readonly rejection?: Json
  }
  readonly failure?: JsonObject
}

export interface OperationSeries {
  readonly operationId: string
  readonly correlationId: string
  readonly mode: 'test' | 'dispatch'
  readonly messageSeries: {
    readonly messages: readonly Message[]
    readonly commandOutcomes: readonly CommandOutcome[]
  }
  readonly capture: {
    readonly settled: boolean
    readonly settledFor: string
    readonly fidelity: 'exact' | 'grouped'
    readonly note?: string
  }
}

export interface HandlerSimulation {
  readonly kind: 'handlerSimulation'
  readonly committed: false
  readonly context: string
  readonly command: string
  readonly schemaVersion: number
  readonly outcome: { readonly decision: 'accepted' | 'rejected'; readonly rejection?: Json }
  readonly streams: readonly {
    readonly aggregateType: string
    readonly aggregateId: string
    readonly baseStreamVersion: string
    readonly readGuard: boolean
  }[]
  readonly proposedEvents: readonly {
    readonly aggregateType: string
    readonly aggregateId: string
    readonly proposedStreamVersion: string
    readonly eventType: string
    readonly schemaVersion: number
    readonly payload?: Json
  }[]
}

export interface Diagnostic {
  readonly code: string
  readonly severity: 'warning' | 'error'
  readonly path: string
  readonly message: string
  readonly relatedPaths?: readonly string[]
  readonly messageIds?: readonly string[]
  readonly responseMessageIds?: readonly string[]
}

interface BasePresentation {
  /** Per-open identity. Never a domain ID or an authorization credential. */
  readonly snapshotId: string
  readonly source: JsonObject
  /** Exact original text, or a serialization when the caller supplied an object. */
  readonly sourceText: string
  readonly captureText?: string
  readonly provenance: 'supported' | 'unknown'
  readonly diagnostics: readonly Diagnostic[]
  readonly warnings: readonly string[]
}

export type Presentation =
  | (BasePresentation & {
    readonly kind: 'operation'
    readonly operation: Operation
    readonly series?: OperationSeries
    readonly captureStatus: 'valid' | 'anomalous' | 'invalid' | 'missing'
    readonly anomalousMessageIds: readonly string[]
    readonly unassociatedOutcomeIds: readonly string[]
    readonly edges: readonly { readonly parentMessageId: string; readonly childMessageId: string }[]
    readonly report?: JsonObject
    readonly runState?: string
    readonly captureState?: string
  })
  | (BasePresentation & { readonly kind: 'handlerSimulation'; readonly simulation: HandlerSimulation })
  | (BasePresentation & { readonly kind: 'incomplete'; readonly runState: string })

export interface Selection {
  readonly snapshotId: string
  readonly operationId: string
  readonly messageId: string
}

export interface ResolvedSelection {
  readonly snapshotId: string
  readonly operationId: string
  readonly message: Message
  readonly outcome?: CommandOutcome
  readonly diagnostics: readonly Diagnostic[]
  readonly path: string
}

export interface RenderOptions {
  readonly includePayloads?: boolean
  readonly selection?: Selection
  /** 512..65536 characters; default 16000. Omission is always explicit. */
  readonly maxCharacters?: number
}
