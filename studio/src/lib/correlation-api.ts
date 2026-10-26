import { TracerResponseError, advertisedHref, request } from "@/lib/api"
import type {
  AnchorCorrelationView,
  CorrelationAnchor,
  CorrelationCapability,
  CorrelationChain,
  CorrelationFreshness,
  CorrelationFreshnessSource,
  CorrelationRef,
  IndexedCorrelationChain,
  TracerCatalog,
} from "@/lib/types"

export const DEFAULT_CORRELATION_EVENTS_TEMPLATE =
  "/correlations/{correlationId}/events"

export type CorrelationFailure =
  | "credential-missing"
  | "forbidden"
  | "unknown-anchor"
  | "unknown-anchor-stream"
  | "not-indexed"
  | "index-unavailable"
  | "invalid-identifier"
  | "request-failed"

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
  includedContexts: string[] = []
): string {
  const query = includedContexts
    .map((context) => `include=context:${encodeURIComponent(context)}`)
    .join("&")
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
    freshness: parseFreshness(body),
    correlations: body.correlations.flatMap((item) => {
      const correlation = parseChain(record(item))
      return correlation ? [correlation] : []
    }),
    truncated: body.truncated === true,
  }
}

export async function getCorrelationChain(
  correlationId: string,
  signal: AbortSignal,
  template = DEFAULT_CORRELATION_EVENTS_TEMPLATE
): Promise<IndexedCorrelationChain> {
  const body = await inspect(
    advertisedHref(
      template.replace("{correlationId}", encodeURIComponent(correlationId))
    ),
    signal
  )
  const chain = parseChain(body)
  if (!chain) throw new Error("Tracer did not return an indexed correlation")
  return { ...chain, freshness: parseFreshness(body) }
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
      return "not-indexed"
    case "invalid-identifier":
    case "include-too-broad":
      return "invalid-identifier"
    case "correlation-index-unavailable":
    case "history-unavailable":
    case "correlation-index-read-failed":
    case "history-read-failed":
      return "index-unavailable"
  }
  return error.status === 404 ? "not-indexed" : "request-failed"
}

// Freshness is read in one place so a change of its wire shape stays local.
// Accepts the current `lag: { context: behind }` map, `lag` values of null, and
// a `freshness` (or object-valued `lag`) with `lag` and `sources`.
export function parseFreshness(
  body: Record<string, unknown>
): CorrelationFreshness {
  const container = record(body.freshness) ?? record(body.lag)
  const nested =
    container && ("sources" in container || isLagMap(container.lag))
  const lag =
    (nested && isLagMap(container.lag)
      ? record(container.lag)
      : isLagMap(body.lag)
        ? record(body.lag)
        : undefined) ?? {}
  const sources: CorrelationFreshnessSource[] = (
    nested && Array.isArray(container.sources) ? container.sources : []
  ).flatMap((item) => {
    const source = record(item)
    const context = text(source?.context)
    if (!source || !context) return []
    return [
      {
        context,
        kind: text(source.kind),
        stream: text(source.stream),
        state: text(source.state),
        behind: count(source.behind),
      },
    ]
  })
  const names = new Set([
    ...Object.keys(lag),
    ...sources.map((source) => source.context),
  ])
  return {
    contexts: [...names].sort().map((context) => {
      const own = sources.filter((source) => source.context === context)
      const behind =
        context in lag
          ? count(lag[context])
          : own.length && own.every((source) => source.behind !== null)
            ? own.reduce((total, source) => total + (source.behind ?? 0), 0)
            : null
      return {
        context,
        behind: own.some((source) => source.state === "inactive")
          ? null
          : behind,
        sources: own,
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
      available: value.available !== false,
    }
  }
  return undefined
}

function isLagMap(value: unknown): boolean {
  const map = record(value)
  return Boolean(
    map &&
    Object.values(map).every(
      (behind) => behind === null || typeof behind === "number"
    )
  )
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
