export const observationCapability = {
  application: "visual-fixture",
  scope: "test",
  listHref: "/observation/test",
  eventsHref: "/observation/test/events",
}

export function observationFlow(count = 2, scope = "test") {
  const correlationId = "external-browser-interaction"
  const messages = [
    {
      kind: "domain-event",
      messageId: "received",
      correlationId,
      causationId: "unobserved-command",
      observationOrder: 0,
      name: "message-received",
      schemaVersion: 1,
      aggregate: { type: "inbox/conversation", id: "conversation-42" },
      payload: { text: "Visual fixture: Hello from the application" },
    },
    {
      kind: "integration-event",
      messageId: "published",
      correlationId,
      causationId: "received",
      observationOrder: 1,
      name: "message-published",
      schemaVersion: 1,
      payload: { conversation_id: "conversation-42" },
    },
    {
      kind: "domain-event",
      messageId: "delivered",
      correlationId,
      causationId: "published",
      observationOrder: 2,
      name: "delivery-recorded",
      schemaVersion: 1,
      aggregate: { type: "inbox/delivery", id: "delivery-42" },
    },
  ].slice(0, count)
  return {
    id: `${scope}-external-flow`,
    application: "visual-fixture",
    scope,
    generation: "fixture-generation",
    revision: String(count),
    correlationId,
    name: "message-received",
    messageCount: count,
    truncated: false,
    conflicted: false,
    detailHref: `/observation/${scope}/flows/${scope}-external-flow`,
    messageSeries: { messages, commandOutcomes: [] },
    fidelity: "grouped",
    partial: true,
  }
}

export function observationSnapshot(flow = observationFlow()) {
  const {
    messageSeries: _series,
    fidelity: _fidelity,
    partial: _partial,
    ...summary
  } = flow
  return {
    application: flow.application,
    scope: flow.scope,
    generation: flow.generation,
    revision: flow.revision,
    status: "live",
    evictedFlows: "0",
    discardedMessages: "0",
    maximumFlows: 128,
    items: [summary],
  }
}

// A local HTTP fixture exercises fetch streaming, bearer headers, cancellation,
// reconnects, and scope changes through the real browser network stack.
export function observationBackend() {
  const clients = new Map()
  const requests = []
  const flows = {
    test: observationFlow(1),
    production: observationFlow(1, "production"),
  }
  let failing = false
  const frame = (scope) =>
    `event: observation\ndata: ${JSON.stringify(observationSnapshot(flows[scope]))}\n\n`
  return {
    requests,
    clients,
    flows,
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        if (!request.url.startsWith("/api/")) return next()
        const path = request.url.slice(4)
        const token = request.headers.authorization
        const scope = path.includes("/production") ? "production" : "test"
        requests.push({ path, token, method: request.method })
        const json = (status, value) => {
          response.writeHead(status, { "content-type": "application/json" })
          response.end(JSON.stringify(value))
        }
        if (path === "/catalog") {
          const discoveryScope =
            token === "Bearer observation-inspection" ? "production" : "test"
          return json(200, {
            catalogVersion: 1,
            contexts: [],
            observation: [
              {
                ...observationCapability,
                scope: discoveryScope,
                listHref: `/observation/${discoveryScope}`,
                eventsHref: `/observation/${discoveryScope}/events`,
              },
            ],
          })
        }
        const expected =
          scope === "production"
            ? "Bearer observation-inspection"
            : "Bearer observation-control"
        if (token !== expected)
          return json(403, { message: "Wrong observation capability" })
        if (path.endsWith("/events")) {
          if (failing)
            return json(503, { message: "Observer fixture disconnected" })
          response.writeHead(200, {
            "content-type": "text/event-stream",
            "cache-control": "no-store",
          })
          response.write(frame(scope))
          clients.set(response, scope)
          request.on("close", () => clients.delete(response))
        } else if (path === flows[scope].detailHref) {
          json(200, flows[scope])
        } else json(501, { message: `Unexpected observation fixture: ${path}` })
      })
    },
    publish(count, scope = "test") {
      flows[scope] = observationFlow(count, scope)
      for (const [response, clientScope] of clients)
        if (clientScope === scope) response.write(frame(scope))
    },
    snapshot(snapshot) {
      for (const [response, scope] of clients)
        if (scope === snapshot.scope)
          response.write(
            `event: observation\ndata: ${JSON.stringify(snapshot)}\n\n`
          )
    },
    disconnect() {
      failing = true
      for (const response of clients.keys()) response.end()
    },
    recover() {
      failing = false
    },
    close() {
      for (const response of clients.keys()) response.end()
    },
  }
}
