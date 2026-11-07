import { TracerResponseError, advertisedHref, request } from "@/lib/api"
import type {
  AnchorCorrelationView,
  CorrelationAnchor,
  CorrelationCapability,
  CorrelationChain,
  CorrelationSnapshot,
  CorrelationRef,
  RetainedCorrelationChain,
  PayloadMatch,
  TracerCatalog,
} from "@/lib/types"

export const DEFAULT_CORRELATION_EVENTS_TEMPLATE =
  "/correlations/{correlationId}/events"

export type CorrelationFailure =
  | "credential-missing"
  | "forbidden"
  | "unknown-anchor"
  | "unknown-anchor-stream"
  | "not-found"
  | "history-unavailable"
  | "read-failed"
  | "invalid-identifier"
  | "match-requires-include"
  | "invalid-match"
  | "request-failed"

export const MAX_PAYLOAD_MATCHES = 8

export function inspectionConfigured(): boolean {
  return Boolean(import.meta.env.VITE_TRACER_INSPECTION_TOKEN)
}

export async function discoverCorrelations(
  signal: AbortSignal
): Promise<CorrelationCapability | undefined> {
  const response = await request(
    "/catalog",
    { signal: AbortSignal.any([signal, AbortSignal.timeout(12_000)]) },
    "inspection"
  )
  const catalog = (await response.json()) as TracerCatalog
  if (catalog.catalogVersion !== 1)
    throw new Error("Unsupported correlation catalog version")
  const capability = catalog.correlations
  if (!capability) return undefined
  return {
    eventsHrefTemplate:
      typeof capability.eventsHrefTemplate === "string"
        ? capability.eventsHrefTemplate
        : DEFAULT_CORRELATION_EVENTS_TEMPLATE,
    anchors: (Array.isArray(capability.anchors) ? capability.anchors : [])
      .filter(
        (anchor): anchor is CorrelationAnchor =>
          typeof anchor?.aggregateType === "string" &&
          typeof anchor.correlationsHrefTemplate === "string"
      )
      .map((anchor) => ({
        ...anchor,
        label:
          typeof anchor.label === "string"
            ? anchor.label
            : anchor.aggregateType,
      })),
  }
}

export function anchorHref(
  aggregateType: string,
  aggregateId: string,
  template?: string,
  includedContexts: string[] = [],
  matches: PayloadMatch[] = []
): string {
  const query = [
    ...includedContexts.map(
      (context) => `include=context:${encodeURIComponent(context)}`
    ),
    ...matches.map(
      (match) => `match=${encodeURIComponent(formatPayloadMatch(match))}`
    ),
  ].join("&")
  return advertisedHref(
    (
      template ??
      `/aggregates/${encodeURIComponent(aggregateType)}/{aggregateId}/correlations`
    ).replace("{aggregateId}", encodeURIComponent(aggregateId)) +
      (query ? `?${query}` : "")
  )
}

export async function getAnchorCorrelations(
  href: string,
  signal: AbortSignal
): Promise<AnchorCorrelationView> {
  const body = await inspect(href, signal)
  const anchor = record(body.anchor)
  if (!Array.isArray(body.correlations))
    throw new Error("Tracer did not return an anchored correlation list")
  return {
    aggregateType: text(anchor?.aggregate_type) ?? "",
    aggregateId: text(anchor?.aggregate_id) ?? "",
    snapshot: parseSnapshot(body),
    correlations: body.correlations.flatMap((item) => {
      const correlation = parseChain(record(item))
      return correlation ? [correlation] : []
    }),
    truncated: body.truncated === true,
    matched: Array.isArray(body.matched)
      ? body.matched.flatMap((item) => {
          const match = record(item)
          const context = text(match?.context)
          const eventType = text(match?.event_type)
          const field = text(match?.field)
          return match && context && eventType && field
            ? [{ context, eventType, field, value: String(match.value ?? "") }]
            : []
        })
      : undefined,
  }
}

export function formatPayloadMatch(match: PayloadMatch): string {
  return `${match.context}/${match.eventType}.${match.field}=${match.value}`
}

// The raw grammar: <context>/<event-type>.<field>[.<field>…]=<value>.
export function parsePayloadMatch(
  expression: string
): PayloadMatch | undefined {
  const slash = expression.indexOf("/")
  const equals = expression.indexOf("=", slash + 1)
  if (slash <= 0 || equals < 0) return undefined
  const target = expression.slice(slash + 1, equals)
  const dot = target.indexOf(".")
  if (dot <= 0) return undefined
  const match = {
    context: expression.slice(0, slash).trim(),
    eventType: target.slice(0, dot).trim(),
    field: target.slice(dot + 1).trim(),
    value: expression.slice(equals + 1),
  }
  return payloadMatchProblem(match) ? undefined : match
}

export function payloadMatchProblem(match: PayloadMatch): string | undefined {
  if (!match.context || !match.eventType || !match.field)
    return "Context, event type and field are required"
  if (/[\s/]/.test(match.context) || /[\s.=]/.test(match.eventType))
    return "Context and event type cannot contain spaces, dots or slashes"
  const segments = match.field.split(".")
  if (segments.some((segment) => !segment) || /[=\s]/.test(match.field))
    return "Use a dot path such as customer.id"
  if (segments.length > 8) return "At most 8 field segments"
  if (new TextEncoder().encode(match.value).length > 256)
    return "Values are limited to 256 bytes"
  return undefined
}

export async function getCorrelationChain(
  correlationId: string,
  signal: AbortSignal,
  template = DEFAULT_CORRELATION_EVENTS_TEMPLATE
): Promise<RetainedCorrelationChain> {
  const body = await inspect(
    advertisedHref(
      template.replace("{correlationId}", encodeURIComponent(correlationId))
    ),
    signal
  )
  const chain = parseChain(body)
  if (!chain)
    throw new Error("Tracer did not return retained correlation events")
  return { ...chain, snapshot: parseSnapshot(body) }
}

export function correlationFailure(error: unknown): CorrelationFailure {
  if (!(error instanceof TracerResponseError))
    return error instanceof Error &&
      /inspection credential is not configured/.test(error.message)
      ? "credential-missing"
      : "request-failed"
  if (error.status === 401 || error.status === 403) return "forbidden"
  switch (error.code) {
    case "unknown-anchor":
      return "unknown-anchor"
    case "unknown-anchor-stream":
      return "unknown-anchor-stream"
    case "not-found":
      return "not-found"
    case "invalid-identifier":
    case "include-too-broad":
      return "invalid-identifier"
    case "match-requires-include":
      return "match-requires-include"
    case "invalid-match":
      return "invalid-match"
    case "correlation-history-unavailable":
      return "history-unavailable"
    case "correlation-read-failed":
      return "read-failed"
  }
  return error.status === 404 ? "not-found" : "request-failed"
}

// Decimal strings preserve sequence values beyond JavaScript's safe integer range.
function parseSnapshot(body: Record<string, unknown>): CorrelationSnapshot {
  const snapshot = record(body.snapshot)
  if (!Array.isArray(snapshot?.sources))
    throw new Error("Tracer did not return correlation source cutoffs")
  return {
    sources: snapshot.sources.map((item) => {
      const source = record(item)
      const stream = text(source?.stream)
      const cutoffSequence = text(source?.cutoff_sequence)
      if (
        !source ||
        (source.kind !== "domain" && source.kind !== "integration") ||
        !stream ||
        !cutoffSequence ||
        !/^\d+$/.test(cutoffSequence) ||
        (source.context !== undefined && !text(source.context))
      )
        throw new Error("Tracer returned invalid correlation source cutoffs")
      return {
        ...(source.context === undefined
          ? {}
          : { context: String(source.context) }),
        kind: source.kind,
        stream,
        cutoffSequence,
      }
    }),
  }
}

async function inspect(
  href: string,
  signal: AbortSignal
): Promise<Record<string, unknown>> {
  const response = await request(
    href,
    { signal: AbortSignal.any([signal, AbortSignal.timeout(15_000)]) },
    "inspection"
  )
  let body: unknown
  try {
    body = await response.json()
  } catch {
    throw new Error("Tracer returned an invalid correlation response")
  }
  const value = record(body)
  if (!value) throw new Error("Tracer returned an invalid correlation response")
  return value
}

function parseChain(
  body: Record<string, unknown> | undefined
): CorrelationChain | undefined {
  const correlationId = text(body?.correlation_id)
  if (!body || !correlationId || !Array.isArray(body.refs)) return undefined
  return {
    correlationId,
    truncated: body.truncated === true,
    refs: body.refs
      .flatMap((item) => {
        const reference = parseRef(record(item))
        return reference ? [reference] : []
      })
      .sort((left, right) => left.occurredAt - right.occurredAt),
  }
}

function parseRef(
  value: Record<string, unknown> | undefined
): CorrelationRef | undefined {
  const context = text(value?.context)
  const eventType = text(value?.event_type)
  const occurredAt = value?.occurred_at
  if (
    !value ||
    !context ||
    !eventType ||
    typeof occurredAt !== "number" ||
    !Number.isFinite(occurredAt)
  )
    return undefined
  const causationId = text(value.causation_id)
  if (value.kind === "domain") {
    const aggregateType = text(value.aggregate_type)
    const aggregateId = text(value.aggregate_id)
    const eventId = text(value.event_id)
    if (!aggregateType || !aggregateId || !eventId) return undefined
    return {
      kind: "domain",
      context,
      aggregateType,
      aggregateId,
      streamVersion: count(value.stream_version) ?? 0,
      eventId,
      eventType,
      causationId,
      occurredAt,
      payload: "payload" in value ? value.payload : undefined,
    }
  }
  if (value.kind === "integration") {
    const messageId = text(value.message_id)
    if (!messageId) return undefined
    return {
      kind: "integration",
      context,
      subject: text(value.subject) ?? "",
      messageId,
      eventType,
      schemaVersion: count(value.schema_version) ?? 0,
      causationId,
      occurredAt,
    }
  }
  return undefined
}

function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value ? value : undefined
}

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : null
}
