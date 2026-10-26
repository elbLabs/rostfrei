export const correlationCapability = {
  eventsHrefTemplate: "/correlations/{correlationId}/events",
  anchors: [
    {
      aggregateType: "bike-rental/rental-fleet",
      label: "Rental fleet",
      correlationsHrefTemplate:
        "/aggregates/bike-rental%2Frental-fleet/{aggregateId}/correlations",
    },
  ],
}

export const anchorPath =
  "/aggregates/bike-rental%2Frental-fleet/city-fleet/correlations"
export const missingAnchorPath =
  "/aggregates/bike-rental%2Frental-fleet/bike-42/correlations"

const start = Date.parse("2026-09-10T11:58:00.000Z")

function domain(
  context,
  aggregate,
  id,
  version,
  event,
  cause,
  offset,
  payload
) {
  return {
    kind: "domain",
    context,
    aggregate_type: aggregate,
    aggregate_id: id,
    stream_version: version,
    event_id: `event:${event}`,
    event_type: event.replace(/-\d+$/, ""),
    ...(cause ? { causation_id: cause } : {}),
    occurred_at: start + offset,
    ...(payload ? { payload } : {}),
  }
}

function integration(context, event, cause, offset, available = true) {
  return {
    kind: "integration",
    context,
    subject: `${context}.integration.${context}.${event}`,
    message_id: `message:${event}`,
    event_type: event,
    schema_version: 1,
    causation_id: cause,
    occurred_at: start + offset,
    available,
  }
}

// Two bounded contexts: the rental starts in bike-rental, billing consumes its
// integration event through a command that is not indexed.
export function correlationChains() {
  return [
    {
      correlation_id: "visual-add-bike-7",
      truncated: false,
      refs: [
        domain(
          "bike-rental",
          "bike-rental/rental-fleet",
          "city-fleet",
          3,
          "bicycle-added",
          "command:add-bike-7",
          0,
          {
            fleet_id: "city-fleet",
            bicycle_id: "bike-7",
            condition: "serviceable",
          }
        ),
      ],
    },
    {
      correlation_id: "visual-rent-bike-42",
      truncated: false,
      refs: [
        domain(
          "bike-rental",
          "bike-rental/rental-fleet",
          "city-fleet",
          4,
          "bicycle-rented",
          "command:rent-bike-42",
          60_000,
          { fleet_id: "city-fleet", bicycle_id: "bike-42" }
        ),
        integration(
          "bike-rental",
          "bicycle-rental-started",
          "event:bicycle-rented",
          60_015
        ),
        domain(
          "billing",
          "billing/invoice",
          "invoice-42",
          1,
          "invoice-opened",
          "command:open-invoice-42",
          60_240
        ),
        integration(
          "billing",
          "invoice-issued",
          "event:invoice-opened",
          60_260
        ),
      ],
    },
    {
      correlation_id: "visual-transfer-bike-7",
      truncated: false,
      refs: [
        domain(
          "bike-rental",
          "bike-rental/rental-fleet",
          "city-fleet",
          5,
          "bicycle-transferred-out",
          "command:transfer-bike-7",
          120_000,
          { fleet_id: "city-fleet", bicycle_id: "bike-7" }
        ),
        domain(
          "bike-rental",
          "bike-rental/rental-fleet",
          "harbor-fleet",
          2,
          "bicycle-transferred-in",
          "command:transfer-bike-7",
          120_000
        ),
      ],
    },
  ]
}

export function anchorView(lag = { "bike-rental": 0, billing: null }) {
  return {
    anchor: {
      aggregate_type: "bike-rental/rental-fleet",
      aggregate_id: "city-fleet",
    },
    lag,
    correlations: correlationChains(),
  }
}

export function chainView(
  correlationId,
  lag = { "bike-rental": 0, billing: null }
) {
  const chain = correlationChains().find(
    (item) => item.correlation_id === correlationId
  )
  if (!chain) return undefined
  return {
    ...chain,
    lag,
    refs: chain.refs.map((reference) => {
      const indexed = { ...reference }
      delete indexed.payload
      return indexed
    }),
  }
}

export const correlationErrors = {
  unknownAnchorStream: {
    code: "unknown-anchor-stream",
    message:
      "`bike-rental/rental-fleet` has no stream `bike-42`; anchor IDs are aggregate IDs",
  },
  notIndexed: {
    code: "not-found",
    message: "correlation `visual-not-indexed` is not indexed",
  },
}
