import assert from "node:assert/strict"
import path from "node:path"
import {
  capturePage,
  closeStudioBrowser,
  launchStudioBrowser,
  observePage,
  startStudioServer,
  studioRoot,
} from "./browser.mjs"
import {
  anchorPath,
  anchorView,
  chainView,
  correlationCapability,
  correlationErrors,
  missingAnchorPath,
} from "./correlation-fixture.mjs"

const INSPECTION = "Bearer correlation-inspection"

// A local HTTP fixture exercises bearer separation, encoded anchor paths and
// Tracer's error bodies through the real browser network stack.
function correlationBackend() {
  const requests = []
  let lag = { "bike-rental": 0, billing: null }
  return {
    requests,
    setLag(value) {
      lag = value
    },
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        if (!request.url.startsWith("/api/")) return next()
        const [url, query = ""] = request.url.slice(4).split("?")
        const token = request.headers.authorization
        requests.push({ path: url, query, token, method: request.method })
        const json = (status, value) => {
          response.writeHead(status, { "content-type": "application/json" })
          response.end(JSON.stringify(value))
        }
        if (url === "/catalog")
          return json(200, {
            catalogVersion: 1,
            contexts: [],
            ...(token === INSPECTION
              ? { correlations: correlationCapability }
              : {}),
          })
        if (token !== INSPECTION)
          return json(403, {
            code: "forbidden",
            message: "Correlation views require inspection",
          })
        if (url === anchorPath) return json(200, anchorView(lag))
        if (url === missingAnchorPath)
          return json(404, correlationErrors.unknownAnchorStream)
        if (
          url ===
          "/aggregates/bike-rental%2Frental-fleet/empty-fleet/correlations"
        )
          return json(200, { ...anchorView(lag), correlations: [] })
        if (url === "/correlations/visual-not-indexed/events")
          return json(404, correlationErrors.notIndexed)
        const chain = /^\/correlations\/([^/]+)\/events$/.exec(url)
        if (chain) {
          const body = chainView(decodeURIComponent(chain[1]), lag)
          return body
            ? json(200, body)
            : json(404, correlationErrors.notIndexed)
        }
        json(501, { message: `Unexpected correlation fixture: ${url}` })
      })
    },
  }
}

export async function checkCorrelations() {
  const backend = correlationBackend()
  const { server, url } = await startStudioServer({
    configureServer: backend.configureServer,
    define: {
      "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api"),
      "import.meta.env.VITE_TRACER_TOKEN": JSON.stringify(
        "correlation-control"
      ),
      "import.meta.env.VITE_TRACER_INSPECTION_TOKEN": JSON.stringify(
        "correlation-inspection"
      ),
    },
  })
  let browser
  let page
  let diagnostics
  try {
    browser = await launchStudioBrowser()
    page = await browser.newPage()
    diagnostics = observePage(page)
    const errors = []
    page.on("pageerror", (error) => errors.push(error.message))
    await page.setViewport({ width: 1440, height: 900 })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.click('[data-layout="workbench"]')
    await page.click('[aria-label="Open correlations panel"]')
    await page.waitForSelector(
      '[aria-label="Anchor aggregate type"] option[value="bike-rental/rental-fleet"]'
    )
    await page.waitForSelector('.connection-status[data-source="live"]')
    await loadAnchor(page, "city-fleet")
    await page.waitForSelector('[data-correlation-id="visual-rent-bike-42"]')
    assert.deepEqual(
      await page.$$eval("[data-correlation-id]", (rows) =>
        rows.map((row) => row.dataset.correlationId)
      ),
      ["visual-add-bike-7", "visual-rent-bike-42", "visual-transfer-bike-7"],
      "correlations keep the anchor's first-seen order"
    )
    assert.match(
      await text(page, '[data-correlation-id="visual-rent-bike-42"]'),
      /4 events · 2 domain · 2 integration.*2 contexts: bike-rental, billing/
    )

    await page.click('[data-correlation-id="visual-rent-bike-42"]')
    await page.waitForSelector(
      '[data-graph-edge][data-source-id="event:invoice-opened"][data-target-id="message:invoice-issued"][data-edge-relationship="causation"]'
    )
    await page.waitForSelector(
      '[data-graph-edge][data-source-id="event:bicycle-rented"][data-target-id="message:bicycle-rental-started"]'
    )
    assert.deepEqual(
      await page.$$eval("[data-graph-node]", (nodes) =>
        nodes
          .map(
            (node) =>
              `${node.dataset.boundedContext}:${node.dataset.messageKind}:${node.dataset.nodeId}`
          )
          .sort()
      ),
      [
        "bike-rental:domain-event:event:bicycle-rented",
        "bike-rental:integration-event:message:bicycle-rental-started",
        "billing:domain-event:event:invoice-opened",
        "billing:integration-event:message:invoice-issued",
      ]
    )
    assert.equal(
      await page.$$eval("[data-graph-edge]", (edges) => edges.length),
      2,
      "an unindexed command never gets a fabricated edge"
    )
    assert.deepEqual(
      await page.$$eval(
        ".correlation-heading [data-freshness-context]",
        (rows) =>
          rows.map(
            (row) => `${row.dataset.freshnessContext}=${row.dataset.freshness}`
          )
      ),
      ["bike-rental=current", "billing=unknown"]
    )
    assert.match(
      await text(page, ".correlation-heading"),
      /billing.*unknown.*projector may be inactive/s
    )
    await page.click('[data-message-id="event:bicycle-rented"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("city-fleet")
    )
    assert.match(
      await text(page, ".message-inspector"),
      /commands are not indexed/,
      "anchor payloads are merged and unresolved causes are explained"
    )
    await page.click('[data-message-id="event:invoice-opened"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("correlation index stores metadata only")
    )

    await page.click('[data-correlation-id="visual-transfer-bike-7"]')
    await page.waitForSelector(
      '[data-graph-node][data-node-id="event:bicycle-transferred-in"]'
    )
    await page.click('[data-message-id="event:bicycle-transferred-in"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("1 other event shares this cause")
    )

    await page.click('.correlation-include input[type="checkbox"]')
    await page.waitForFunction(
      () => document.querySelector(".correlation-include input")?.checked
    )
    assert.ok(
      backend.requests.some(
        (request) =>
          request.path === anchorPath &&
          request.query === "include=context:billing"
      ),
      "including a context requests its stream payloads"
    )

    backend.setLag({
      lag: { "bike-rental": 3, billing: null },
      sources: [
        {
          context: "billing",
          kind: "domain",
          stream: "BILLING",
          state: "inactive",
          behind: null,
        },
      ],
    })
    await page.click('[aria-label="Refresh anchor correlations"]')
    await page.waitForSelector(
      '.correlation-heading [data-freshness-context="bike-rental"][data-freshness="behind"]'
    )
    assert.match(await text(page, ".correlation-heading"), /3 behind/)
    assert.match(await text(page, ".correlation-heading"), /Projector inactive/)
    backend.setLag({ "bike-rental": 0, billing: null })

    await loadAnchor(page, "empty-fleet")
    await page.waitForFunction(() =>
      document
        .querySelector("#studio-correlation-panel")
        ?.textContent.includes("No correlations for this anchor")
    )

    await loadAnchor(page, "bike-42")
    await page.waitForFunction(() =>
      document
        .querySelector("#studio-correlation-panel [role='alert']")
        ?.textContent.includes("No stream for this anchor ID")
    )
    assert.equal(await page.$("[data-correlation-id]"), null)

    await page.type('[aria-label="Correlation ID"]', "visual-not-indexed")
    await page.click('[aria-label="Open a correlation by ID"] button')
    await page.waitForSelector('[data-correlation-failure="not-indexed"]')
    assert.match(
      await text(page, '[data-correlation-failure="not-indexed"]'),
      /Correlation not indexed yet.*404/s
    )
    assert.equal(
      await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
      0,
      "a failed lookup never shows demo or stale messages"
    )

    const correlationRequests = backend.requests.filter(
      (request) =>
        request.path.startsWith("/aggregates/") ||
        request.path.startsWith("/correlations/")
    )
    assert.ok(correlationRequests.length >= 7)
    assert.ok(
      correlationRequests.every(
        (request) => request.token === INSPECTION && request.method === "GET"
      ),
      "correlation views only use the read-only inspection capability"
    )

    const freshness = await page.evaluate(async () => {
      const { parseFreshness } = await import("/src/lib/correlation-api.ts")
      return [
        parseFreshness({ lag: { a: 0, b: 4 } }),
        parseFreshness({ lag: { a: null } }),
        parseFreshness({}),
        parseFreshness({
          freshness: {
            lag: { a: 0, b: null },
            sources: [
              {
                context: "a",
                kind: "domain",
                stream: "A",
                state: "active",
                behind: 0,
              },
              {
                context: "b",
                kind: "integration",
                stream: "B",
                state: "inactive",
                behind: 2,
              },
              {
                context: "c",
                kind: "domain",
                stream: "C",
                state: "active",
                behind: 5,
              },
            ],
          },
        }),
      ].map((value) =>
        value.contexts.map((entry) => `${entry.context}=${entry.behind}`)
      )
    })
    assert.deepEqual(freshness, [
      ["a=0", "b=4"],
      ["a=null"],
      [],
      ["a=0", "b=null", "c=5"],
    ])

    const graph = await page.evaluate(async () => {
      const { correlationGraph } = await import("/src/lib/graph.ts")
      return correlationGraph([
        {
          kind: "integration",
          context: "conversation",
          subject: "conversation.integration.message-accepted",
          messageId: "root",
          eventType: "message-accepted",
          schemaVersion: 1,
          causationId: "root",
          occurredAt: 1,
          available: true,
        },
        {
          kind: "domain",
          context: "inquiry",
          aggregateType: "inquiry/lifecycle",
          aggregateId: "a",
          streamVersion: 1,
          eventId: "event:a",
          eventType: "inquiry-opened",
          causationId: "root",
          occurredAt: 2,
        },
      ]).map((node) => [
        node.id,
        node.parentId ?? null,
        node.edgeFidelity ?? null,
      ])
    })
    assert.deepEqual(
      graph,
      [
        ["root", null, null],
        ["event:a", "root", "exact"],
      ],
      "a self-caused origin is a root, not a cycle"
    )

    await page.click('[aria-label="Return to executions"]')
    await page.waitForSelector('[aria-label="Open correlations panel"]')
    assert.equal(
      await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
      0
    )
    assert.deepEqual(errors, [])
    console.log(
      "Correlations: anchor discovery, first-seen list, two-context chain, anchor payloads, freshness shapes, unknown anchor stream, empty and unindexed correlations, and inspection-only requests passed"
    )
  } catch (error) {
    if (page && !page.isClosed()) {
      console.log(
        await capturePage(
          page,
          path.join(studioRoot, "artifacts/review/correlation-failure"),
          { diagnostics, source: "mocked" }
        )
      )
    }
    throw error
  } finally {
    await closeStudioBrowser(browser)
    await server.close()
  }
}

async function loadAnchor(page, aggregateId) {
  await page.$eval('[aria-label="Anchor aggregate ID"]', (input) => {
    const setter = Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value"
    ).set
    setter.call(input, "")
    input.dispatchEvent(new Event("input", { bubbles: true }))
  })
  await page.type('[aria-label="Anchor aggregate ID"]', aggregateId)
  await page.click('#studio-correlation-panel button[type="submit"]')
}

async function text(page, selector) {
  return page.$eval(selector, (element) => element.textContent ?? "")
}

if (process.argv[1]?.endsWith("correlation-smoke.mjs"))
  await checkCorrelations()
