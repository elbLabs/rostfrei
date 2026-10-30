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

function integration(context, event, cause, offset) {
  return {
    kind: "integration",
    context,
    subject: `${context}.integration.${context}.${event}`,
    message_id: `message:${event}`,
    event_type: event,
    schema_version: 1,
    causation_id: cause,
    occurred_at: start + offset,
  }
}

// Command causes are outside retained event history.
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

export const snapshotSources = [
  {
    context: "bike-rental",
    kind: "domain",
    stream: "BIKE_RENTAL__BIKE_RENTAL_DOMAIN_EVENTS",
    cutoff_sequence: "9007199254740993",
  },
  {
    context: "billing",
    kind: "domain",
    stream: "BIKE_RENTAL__BILLING_DOMAIN_EVENTS",
    cutoff_sequence: "18446744073709551615",
  },
  {
    kind: "integration",
    stream: "BIKE_RENTAL_INTEGRATION_EVENTS",
    cutoff_sequence: "42",
  },
]

export function correlationSnapshot() {
  return { sources: structuredClone(snapshotSources) }
}

export function anchorView(snapshot = correlationSnapshot()) {
  return {
    anchor: {
      aggregate_type: "bike-rental/rental-fleet",
      aggregate_id: "city-fleet",
    },
    snapshot,
    truncated: false,
    correlations: correlationChains(),
  }
}

const included = {
  "bike-rental": {
    "event:bicycle-transferred-in": {
      fleet_id: "harbor-fleet",
      bicycle_id: "bike-7",
    },
  },
  billing: {
    "event:invoice-opened": {
      invoice_id: "invoice-42",
      amount: 12,
      paid: false,
    },
  },
}

function fieldValue(payload, path) {
  return path
    .split(".")
    .reduce(
      (value, segment) =>
        value === null || typeof value !== "object"
          ? undefined
          : value[segment],
      payload
    )
}

function equals(actual, expected) {
  if (typeof actual === "string") return actual === expected
  if (typeof actual === "number")
    return expected.trim() !== "" && Number(expected) === actual
  if (typeof actual === "boolean") return String(actual) === expected
  return false
}

export function anchorResponse(search, snapshot) {
  const params = new URLSearchParams(search)
  const contexts = params
    .getAll("include")
    .map((value) => value.replace(/^context:/, ""))
  const matches = []
  for (const expression of params.getAll("match")) {
    const slash = expression.indexOf("/")
    const equalsAt = expression.indexOf("=", slash + 1)
    const target = expression.slice(slash + 1, equalsAt)
    const dot = target.indexOf(".")
    if (slash <= 0 || equalsAt < 0 || dot <= 0)
      return {
        status: 400,
        body: {
          code: "invalid-match",
          message: `invalid match \`${expression}\``,
        },
      }
    const match = {
      context: expression.slice(0, slash),
      event_type: target.slice(0, dot),
      field: target.slice(dot + 1),
      value: expression.slice(equalsAt + 1),
    }
    if (match.context !== "bike-rental" && !contexts.includes(match.context))
      return {
        status: 400,
        body: {
          code: "match-requires-include",
          message: `payloads of context \`${match.context}\` are not loaded; add include=context:${match.context}`,
        },
      }
    matches.push(match)
  }
  const view = anchorView(snapshot)
  view.correlations = view.correlations.map((correlation) => ({
    ...correlation,
    refs: correlation.refs.map((reference) => {
      const payload = contexts.includes(reference.context)
        ? included[reference.context]?.[reference.event_id]
        : undefined
      return payload ? { ...reference, payload } : reference
    }),
  }))
  if (!matches.length) return { status: 200, body: view }
  view.matched = matches
  view.correlations = view.correlations.filter((correlation) =>
    matches.every((match) =>
      correlation.refs.some(
        (reference) =>
          reference.kind === "domain" &&
          reference.context === match.context &&
          reference.event_type === match.event_type &&
          reference.payload !== undefined &&
          equals(fieldValue(reference.payload, match.field), match.value)
      )
    )
  )
  return { status: 200, body: view }
}

export function chainView(correlationId, snapshot = correlationSnapshot()) {
  if (correlationId === "visual-expired-parent")
    return {
      correlation_id: correlationId,
      snapshot,
      truncated: false,
      // The parent integration message has expired. There is no retained ref
      // from which a client could infer its type, payload or expiration reason.
      refs: [
        domain(
          "billing",
          "billing/invoice",
          "old-invoice",
          2,
          "invoice-settled",
          "message:expired-payment",
          180_000
        ),
      ],
    }
  if (correlationId === "visual-truncated-empty")
    return {
      correlation_id: correlationId,
      snapshot,
      truncated: true,
      refs: [],
    }
  if (correlationId === "visual-truncated")
    return {
      ...correlationChains()[1],
      correlation_id: correlationId,
      snapshot,
      truncated: true,
      refs: chainView("visual-rent-bike-42", snapshot).refs.slice(0, 2),
    }
  const chain = correlationChains().find(
    (item) => item.correlation_id === correlationId
  )
  if (!chain) return undefined
  return {
    ...chain,
    snapshot,
    refs: chain.refs.map((reference) => {
      const retained = { ...reference }
      delete retained.payload
      return retained
    }),
  }
}

export const correlationErrors = {
  unknownAnchor: {
    code: "unknown-anchor",
    message: "`billing/not-an-anchor` is not a declared anchor type",
  },
  unknownAnchorStream: {
    code: "unknown-anchor-stream",
    message:
      "`bike-rental/rental-fleet` has no stream `bike-42`; anchor IDs are aggregate IDs",
  },
  notFound: {
    code: "not-found",
    message: "no retained events found for correlation `visual-not-retained`",
  },
  unavailable: {
    code: "correlation-history-unavailable",
    message: "retained correlation history is unavailable",
  },
  readFailed: {
    code: "correlation-read-failed",
    message: "could not read retained correlation events from BILLING",
  },
}
