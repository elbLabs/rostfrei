import assert from "node:assert/strict"
import { mkdir, writeFile } from "node:fs/promises"
import path from "node:path"
import {
  capturePage,
  closeStudioBrowser,
  launchStudioBrowser,
  observePage,
  settlePage,
  startStudioServer,
  studioRoot,
} from "./browser.mjs"
import {
  anchorPath,
  anchorResponse,
  anchorView,
  chainView,
  correlationCapability,
  correlationErrors,
  correlationSnapshot,
  missingAnchorPath,
} from "./correlation-fixture.mjs"

const INSPECTION = "Bearer correlation-inspection"

function correlationBackend() {
  const requests = []
  let snapshot = correlationSnapshot()
  let anchorResult
  const directResults = new Map()
  const holds = new Map()
  const pending = new Set()
  return {
    requests,
    reset() {
      for (const release of pending) release()
      requests.length = 0
      snapshot = correlationSnapshot()
      anchorResult = undefined
      directResults.clear()
      holds.clear()
    },
    setSnapshot(value) {
      snapshot = value
    },
    setAnchorResult(value) {
      anchorResult = value
    },
    setDirectResult(id, value) {
      directResults.set(id, value)
    },
    holdNext(path) {
      let received
      let reply
      const started = new Promise((resolve) => {
        received = resolve
      })
      const release = () => {
        reply?.()
        pending.delete(release)
      }
      holds.set(path, (respond) => {
        reply = respond
        pending.add(release)
        received()
      })
      return { started, release }
    },
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        if (!request.url.startsWith("/api/")) return next()
        const [url, query = ""] = request.url.slice(4).split("?")
        const token = request.headers.authorization
        requests.push({ path: url, query, token, method: request.method })
        const json = (status, value) => {
          const respond = () => {
            response.writeHead(status, { "content-type": "application/json" })
            response.end(JSON.stringify(value))
          }
          const hold = holds.get(url)
          if (hold) {
            holds.delete(url)
            hold(respond)
          } else respond()
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
        if (url === anchorPath) {
          const { status, body } =
            anchorResult ?? anchorResponse(query, snapshot)
          return json(status, body)
        }
        if (url === missingAnchorPath)
          return json(404, correlationErrors.unknownAnchorStream)
        if (
          url ===
          "/aggregates/bike-rental%2Frental-fleet/empty-fleet/correlations"
        )
          return json(200, { ...anchorView(snapshot), correlations: [] })
        if (url.startsWith("/aggregates/billing%2Fnot-an-anchor/"))
          return json(400, correlationErrors.unknownAnchor)
        if (url === "/correlations/visual-not-retained/events")
          return json(404, correlationErrors.notFound)
        const chain = /^\/correlations\/([^/]+)\/events$/.exec(url)
        if (chain) {
          const id = decodeURIComponent(chain[1])
          const result = directResults.get(id)
          if (result) return json(result.status, result.body)
          const body = chainView(id, snapshot)
          return body ? json(200, body) : json(404, correlationErrors.notFound)
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
  const results = []
  const artifactRoot = path.resolve(
    studioRoot,
    process.env.CORRELATION_ARTIFACT_DIR ?? "artifacts/review/correlations"
  )
  try {
    browser = await launchStudioBrowser()
    for (const [name, check] of [
      ["anchor-interactions", checkAnchorInteractions],
      ["payload-matches", checkMatches],
      ["source-cutoffs", checkSourceCutoffs],
      ["snapshot-selection", checkSnapshotSelection],
      ["retention-evidence", checkRetentionEvidence],
      [
        "history-unavailable-501",
        (page, backend) => checkReadFailures(page, backend, 501),
      ],
      [
        "read-failed-503",
        (page, backend) => checkReadFailures(page, backend, 503),
      ],
      [
        "not-retained-404",
        (page, backend) => checkReadFailures(page, backend, 404),
      ],
      ["reading-retained-events", checkReadingRetainedEvents],
      ["truncated-evidence", checkTruncatedEvidence],
      ["selection-race", checkSelectionRace],
      ["filtered-selection", checkFilteredSelection],
      ["same-context-includes", checkSameContextIncludes],
      ["mobile-layouts", checkMobileLayouts],
    ]) {
      backend.reset()
      const context = await browser.createBrowserContext()
      const page = await context.newPage()
      page.setDefaultTimeout(10_000)
      page.setDefaultNavigationTimeout(30_000)
      const diagnostics = observePage(page)
      try {
        await page.setViewport({ width: 1440, height: 900 })
        await page.goto(url, { waitUntil: "domcontentloaded" })
        await page.waitForSelector('[data-layout="workbench"]')
        await page.click('[data-layout="workbench"]')
        await page.click('[aria-label="Open correlations panel"]')
        await page.waitForSelector(
          '[aria-label="Anchor aggregate type"] option[value="bike-rental/rental-fleet"]'
        )
        await page.waitForSelector('.connection-status[data-source="live"]')
        await check(page, backend)
        assert.deepEqual(diagnostics.pageErrors, [])
        results.push({ name, status: "passed" })
        console.log(`Correlations: ${name} passed`)
      } catch (error) {
        const evidence = await capturePage(
          page,
          path.join(artifactRoot, name),
          {
            diagnostics,
            source: "mocked",
            scene: name,
            inspectionError: error.message,
          }
        )
        results.push({ name, status: "failed", error: error.message, evidence })
        console.error(`Correlations: ${name} FAILED\n${error.message}`)
      } finally {
        backend.reset()
        await context.close()
      }
    }
    await mkdir(artifactRoot, { recursive: true })
    await writeFile(
      path.join(artifactRoot, "results.json"),
      JSON.stringify(results, null, 2) + "\n"
    )
    assert.equal(
      results.filter((result) => result.status === "failed").length,
      0,
      `Correlation browser failures; see ${artifactRoot}/results.json`
    )
  } finally {
    backend.reset()
    await closeStudioBrowser(browser)
    await server.close()
  }
}

async function checkAnchorInteractions(page, backend) {
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
    "a cause outside retained history never gets a fabricated edge"
  )
  await page.click('[data-message-id="event:bicycle-rented"]')
  await page.waitForFunction(() =>
    document
      .querySelector(".message-inspector")
      ?.textContent.includes("city-fleet")
  )
  assert.match(
    await text(page, ".message-inspector"),
    /cause.*(?:not (?:in|returned)|outside).*retained|cause.*not returned/is,
    "anchor payloads remain available and absent causes are explained without inferring their type"
  )
  await page.click('[data-message-id="event:invoice-opened"]')
  await page.waitForFunction(() =>
    document
      .querySelector(".message-inspector")
      ?.textContent.includes("Not loaded")
  )
  assert.doesNotMatch(
    await text(page, ".message-inspector"),
    /index|projector/i
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

  await page.click("::-p-aria(billing)")
  await page.waitForFunction(
    () =>
      [...document.querySelectorAll(".correlation-include label")]
        .find((label) => label.textContent.trim() === "billing")
        ?.querySelector("input")?.checked
  )
  assert.ok(
    backend.requests.some(
      (request) =>
        request.path === anchorPath &&
        request.query === "include=context:billing"
    ),
    "including a context requests its stream payloads"
  )

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

  await page.type('[aria-label="Correlation ID"]', "visual-not-retained")
  await page.click('[aria-label="Open a correlation by ID"] button')
  await page.waitForSelector('[data-correlation-failure="not-found"]')
  assert.match(
    await text(page, '[data-correlation-failure="not-found"]'),
    /No retained events.*404/s
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
  assert.ok(correlationRequests.length >= 5)
  assert.ok(
    correlationRequests.every(
      (request) => request.token === INSPECTION && request.method === "GET"
    ),
    "correlation views only use the read-only inspection capability"
  )

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
}

async function checkSourceCutoffs(page, backend) {
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-rent-bike-42"]')
  const footer = await text(page, ".correlation-footer")
  for (const source of correlationSnapshot().sources) {
    assert.ok(
      footer.includes(source.cutoff_sequence),
      `source cutoff ${source.cutoff_sequence} must be displayed exactly; actual: ${footer}`
    )
    assert.ok(
      footer.includes(source.stream),
      `source stream ${source.stream} must be identifiable`
    )
  }
  assert.match(footer, /3 sources/)
  assert.match(footer, /independen/i)
  assert.match(footer, /not.*(?:atomic|simultaneous)/i)
  assert.match(footer, /not.*(?:complet|finish)/i)
  assert.doesNotMatch(footer, /projector|\blag\b|behind|skipped|inactive/i)
  await page.click('[data-correlation-id="visual-rent-bike-42"]')
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-rented"]'
  )

  const next = correlationSnapshot()
  next.sources[0].cutoff_sequence = "9007199254740995"
  next.sources[2].cutoff_sequence = "0"
  backend.setSnapshot(next)
  await page.click('[aria-label="Refresh anchor correlations"]')
  await page.waitForFunction(() =>
    document
      .querySelector(".correlation-footer")
      ?.textContent.includes("9007199254740995")
  )
  assert.ok(
    !(await text(page, ".correlation-footer")).includes("9007199254740993")
  )
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    4
  )

  const parsed = await page.evaluate(async (href) => {
    const { getAnchorCorrelations } =
      await import("/src/lib/correlation-api.ts")
    return (await getAnchorCorrelations(href, new AbortController().signal))
      .snapshot
  }, anchorPath)
  assert.deepEqual(
    parsed?.sources?.map((source) => [
      source.context ?? null,
      source.kind,
      source.stream,
      source.cutoffSequence,
    ]),
    next.sources.map((source) => [
      source.context ?? null,
      source.kind,
      source.stream,
      source.cutoff_sequence,
    ])
  )
}

async function checkSnapshotSelection(page, backend) {
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-rent-bike-42"]')
  const later = correlationSnapshot()
  later.sources[0].cutoff_sequence = "9007199254740995"
  backend.setSnapshot(later)
  const before = backend.requests.length
  await page.click('[data-correlation-id="visual-rent-bike-42"]')
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-rented"]'
  )
  await settlePage(page)
  assert.deepEqual(
    backend.requests.slice(before),
    [],
    "selecting an already-returned anchor chain must use that response, not fetch and merge a different snapshot"
  )
  await page.click('[data-message-id="event:bicycle-rented"]')
  assert.match(await text(page, ".message-inspector"), /city-fleet/)
  assert.ok(
    (await text(page, ".correlation-heading")).includes("9007199254740993")
  )
  assert.ok(
    !(await text(page, ".correlation-heading")).includes("9007199254740995")
  )
  await page.click('[data-layout="canvas"]')
  await page.waitForSelector(".message-inspector", { visible: true })
  assert.match(await text(page, ".message-inspector"), /city-fleet/)
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    4
  )
  await page.click('[data-layout="workbench"]')
  assert.match(await text(page, ".message-inspector"), /city-fleet/)
}

async function checkRetentionEvidence(page) {
  await openCorrelation(page, "visual-expired-parent")
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:invoice-settled"]'
  )
  assert.deepEqual(
    await page.$$eval("[data-graph-node]", (nodes) =>
      nodes.map((node) => node.dataset.nodeId)
    ),
    ["event:invoice-settled"],
    "an expired integration message is absent, never a fabricated metadata node"
  )
  assert.equal(
    await page.$$eval("[data-graph-edge]", (edges) => edges.length),
    0
  )
  await page.click('[data-message-id="event:invoice-settled"]')
  const inspector = await text(page, ".message-inspector")
  assert.match(
    inspector,
    /cause.*(?:not (?:in|returned)|outside).*retained|cause.*not returned/is
  )
  assert.match(inspector, /Not loaded/)
  assert.doesNotMatch(
    inspector,
    /index|projector|commands are|Stream retentionExpired/i,
    "an absent parent's type and reason for absence cannot be inferred from its ID"
  )
  assert.match(await text(page, ".flow-heading"), /Retained correlation events/)
}

async function checkReadFailures(page, backend, testedStatus) {
  // Seed a successful graph to detect stale evidence after a failed request.
  for (const [status, body, title] of [
    [501, correlationErrors.unavailable, /Correlation history unavailable/],
    [503, correlationErrors.readFailed, /Correlation read failed/],
    [404, correlationErrors.notFound, /No retained events/],
  ].filter(([status]) => status === testedStatus)) {
    await openCorrelation(page, "visual-rent-bike-42")
    await page.waitForSelector(
      '[data-graph-node][data-node-id="event:bicycle-rented"]'
    )
    backend.setDirectResult("visual-error", { status, body })
    await openCorrelation(page, "visual-error")
    await page.waitForSelector("[data-correlation-failure]")
    const actual = await text(page, "[data-correlation-failure]")
    assert.match(
      actual,
      title,
      `HTTP ${status} must keep its retained-history meaning`
    )
    assert.ok(actual.includes(String(status)))
    assert.equal(
      await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
      0
    )
    assert.doesNotMatch(actual, /index|projector|catch up|no source is behind/i)
    if (status !== 404)
      assert.doesNotMatch(actual, /No retained events|No correlations for/)
  }
  for (const [status, body] of [
    [501, correlationErrors.unavailable],
    [503, correlationErrors.readFailed],
  ]) {
    if (status !== testedStatus) continue
    backend.setAnchorResult({ status, body })
    await loadAnchor(page, "city-fleet")
    await page.waitForSelector("#studio-correlation-panel [role='alert']")
    assert.equal(await page.$("[data-correlation-id]"), null)
    const actual = await text(page, "#studio-correlation-panel [role='alert']")
    assert.ok(actual.includes(String(status)))
    assert.doesNotMatch(actual, /No correlations for|index|projector/i)
  }
  backend.setAnchorResult(undefined)
  await page.select('[aria-label="Anchor aggregate type"]', "")
  await setValue(
    page,
    '[aria-label="Other anchor aggregate type"]',
    "billing/not-an-anchor"
  )
  await loadAnchor(page, "invoice-42")
  await page.waitForFunction(() =>
    document
      .querySelector("#studio-correlation-panel [role='alert']")
      ?.textContent.includes("Not an anchor aggregate")
  )
}

async function checkReadingRetainedEvents(page, backend) {
  const waiting = backend.holdNext(
    "/correlations/visual-truncated-empty/events"
  )
  await openCorrelation(page, "visual-truncated-empty")
  await waiting.started
  assert.match(
    await text(page, ".correlation-heading"),
    /Reading retained events/
  )
  waiting.release()
}

async function checkTruncatedEvidence(page, backend) {
  await openCorrelation(page, "visual-truncated-empty")
  await page.waitForSelector('.correlation-heading[aria-busy="false"]')
  const empty = await text(page, ".correlation-heading")
  assert.match(empty, /truncat|incomplete/i)
  assert.doesNotMatch(empty, /indexed|catch up|2048|No retained events found/i)
  assert.equal(
    await page.$("[data-correlation-failure]"),
    null,
    "a successful truncated empty read is incomplete evidence, not 404"
  )
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    0
  )
  await openCorrelation(page, "visual-truncated")
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-rented"]'
  )
  assert.match(await text(page, ".correlation-heading"), /truncat|incomplete/i)
  assert.doesNotMatch(
    await text(page, ".correlation-heading"),
    /2048/,
    "the UI cannot invent the resource bound that truncated a native read"
  )
  const view = anchorView()
  view.truncated = true
  view.correlations[1].truncated = true
  backend.setAnchorResult({ status: 200, body: view })
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-rent-bike-42"]')
  assert.match(
    await text(page, ".correlation-list-heading"),
    /truncat|incomplete/i
  )
  assert.doesNotMatch(
    await text(page, ".correlation-list-heading"),
    /older ones omitted/,
    "a resource bound does not establish which events were omitted"
  )
  assert.match(
    await text(page, '[data-correlation-id="visual-rent-bike-42"]'),
    /truncated/i
  )
}

async function checkSelectionRace(page, backend) {
  const delayed = backend.holdNext("/correlations/visual-rent-bike-42/events")
  await openCorrelation(page, "visual-rent-bike-42")
  await delayed.started
  const lateResponse = page
    .waitForResponse((response) =>
      response.url().includes("/correlations/visual-rent-bike-42/events")
    )
    .catch(() => undefined)
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-transfer-bike-7"]')
  delayed.release()
  await lateResponse
  await settlePage(page)
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    0,
    "a delayed direct-ID response cannot replace a newer anchor search"
  )
  assert.doesNotMatch(
    await text(page, ".correlation-heading"),
    /visual-rent-bike-42/
  )
}

async function checkFilteredSelection(page, backend) {
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-transfer-bike-7"]')
  await page.click("::-p-text(Add match)")
  await setValue(page, '[aria-label="Match 1 context"]', "bike-rental")
  await setValue(page, '[aria-label="Match 1 event type"]', "bicycle-rented")
  await setValue(page, '[aria-label="Match 1 field"]', "bicycle_id")
  await setValue(page, '[aria-label="Match 1 value"]', "bike-42")
  await page.click('#studio-correlation-panel button[type="submit"]')
  await page.waitForSelector(
    '[data-matched="bike-rental/bicycle-rented.bicycle_id=bike-42"]'
  )
  await page.click('[data-correlation-id="visual-rent-bike-42"]')
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-rented"]'
  )
  // A later redacted response must clear previously exposed payloads.
  const filtered = anchorResponse(
    "match=bike-rental/bicycle-rented.bicycle_id=bike-42"
  )
  filtered.body.correlations = []
  backend.setAnchorResult(filtered)
  await page.click('[aria-label="Refresh anchor correlations"]')
  await page.waitForFunction(
    () => !document.querySelector("[data-correlation-id]")
  )
  await settlePage(page)
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    0,
    "a refreshed/filtered anchor result must clear a selected chain absent from that response"
  )
  assert.doesNotMatch(
    await text(page, ".correlation-heading"),
    /visual-rent-bike-42/
  )
}

async function checkSameContextIncludes(page, backend) {
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-transfer-bike-7"]')
  const labels = await page.$$eval(".correlation-include label", (items) =>
    items.map((item) => item.textContent.trim())
  )
  assert.ok(
    labels.includes("bike-rental"),
    `other aggregates in the anchor's context also need an include control; actual: ${labels}`
  )
  await page.click('[data-correlation-id="visual-transfer-bike-7"]')
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-transferred-in"]'
  )
  await page.click('[data-message-id="event:bicycle-transferred-in"]')
  assert.match(await text(page, ".message-inspector"), /Not loaded/)
  await page.click("::-p-aria(bike-rental)")
  await page.waitForFunction(() =>
    document.querySelector(".message-inspector")?.textContent.includes("bike-7")
  )
  assert.ok(
    backend.requests.some(
      (request) =>
        request.path === anchorPath &&
        request.query === "include=context:bike-rental"
    )
  )
  assert.match(await text(page, ".message-inspector"), /harbor-fleet.*bike-7/s)
}

async function checkMobileLayouts(page) {
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-rent-bike-42"]')
  await page.click('[data-correlation-id="visual-rent-bike-42"]')
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-rented"]'
  )
  await page.setViewport({ width: 390, height: 844 })
  if (await page.$('[aria-label="Collapse sidebar"]'))
    await page.click('[aria-label="Collapse sidebar"]')
  await page.waitForSelector(".message-list")
  assert.equal(
    await page.$$eval(".message-list > li", (nodes) => nodes.length),
    4
  )
  assert.match(await text(page, ".flow-heading"), /Retained correlation events/)
  await page.click('[data-message-id="event:invoice-opened"]')
  await page.waitForSelector(".message-inspector", { visible: true })
  assert.match(await text(page, ".message-inspector"), /invoice-opened/)
  await settlePage(page)
  const bounds = await page.$eval(".message-inspector", (element) => {
    const rect = element.getBoundingClientRect()
    return [
      rect.left,
      rect.right,
      rect.bottom,
      innerWidth,
      innerHeight,
      document.documentElement.scrollWidth,
    ]
  })
  assert.ok(
    bounds[0] >= 0 &&
      bounds[1] <= bounds[3] &&
      bounds[2] <= bounds[4] &&
      bounds[5] === bounds[3],
    `mobile details must fit the viewport: ${bounds}`
  )
  await page.click("::-p-aria(Back to flow)")
  await page.waitForSelector(".message-list", { visible: true })
}

async function openCorrelation(page, id) {
  await setValue(page, '[aria-label="Correlation ID"]', id)
  await page.click('[aria-label="Open a correlation by ID"] button')
}

async function checkMatches(page, backend) {
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector('[data-correlation-id="visual-rent-bike-42"]')
  const billing = "::-p-aria(billing)"
  if (await page.$eval(billing, (input) => input.checked))
    await page.click(billing)
  await page.waitForFunction(
    () =>
      ![...document.querySelectorAll(".correlation-include label")]
        .find((label) => label.textContent.trim() === "billing")
        .querySelector("input").checked &&
      !document.querySelector(".correlation-include").disabled
  )
  const before = backend.requests.length
  await page.click("::-p-text(Add match)")
  await setValue(page, '[aria-label="Match 1 context"]', "billing")
  await setValue(page, '[aria-label="Match 1 event type"]', "invoice-opened")
  await setValue(page, '[aria-label="Match 1 field"]', "amount..x")
  await page.waitForSelector('[data-match-row="1"] small[role="alert"]')
  assert.ok(
    await page.$eval(
      '#studio-correlation-panel button[type="submit"]',
      (button) => button.disabled
    ),
    "an invalid match row blocks the search"
  )
  await setValue(page, '[aria-label="Match 1 field"]', "amount")
  await setValue(page, '[aria-label="Match 1 value"]', "12")
  await page.click('#studio-correlation-panel button[type="submit"]')
  await page.waitForSelector(
    '[data-matched="billing/invoice-opened.amount=12"]'
  )
  assert.deepEqual(
    backend.requests
      .slice(before)
      .filter((request) => request.path === anchorPath)
      .at(-1).query,
    "include=context:billing&match=billing%2Finvoice-opened.amount%3D12",
    "match rows become include and form-encoded match parameters"
  )
  assert.match(
    await text(page, ".match-note"),
    /Included billing so its payloads can be matched/
  )
  assert.ok(await page.$eval(billing, (input) => input.checked))
  assert.deepEqual(
    await page.$$eval("[data-correlation-id]", (rows) =>
      rows.map((row) => row.dataset.correlationId)
    ),
    ["visual-rent-bike-42"]
  )

  await page.reload({ waitUntil: "networkidle0" })
  await page.click('[aria-label="Open correlations panel"]')
  await page.waitForFunction(
    () =>
      document.querySelector('[aria-label="Match 1 context"]')?.value ===
      "billing"
  )
  assert.equal(
    await page.$eval('[aria-label="Match 1 value"]', (input) => input.value),
    "12",
    "the last match rows are restored for the anchor"
  )
  await page.click(".match-raw summary")
  await setValue(page, '[aria-label="Raw match expression"]', "bike-rental")
  await page.click(".match-raw button")
  await page.waitForSelector('.match-raw small[role="alert"]')
  await setValue(
    page,
    '[aria-label="Raw match expression"]',
    "bike-rental/bicycle-rented.bicycle_id=bike-42"
  )
  await page.click(".match-raw button")
  await page.waitForFunction(
    () =>
      document.querySelector('[aria-label="Match 2 field"]')?.value ===
      "bicycle_id"
  )
  await loadAnchor(page, "city-fleet")
  await page.waitForSelector(
    '[data-matched="bike-rental/bicycle-rented.bicycle_id=bike-42"]'
  )
  assert.equal(await page.$$eval("[data-matched]", (chips) => chips.length), 2)
  await page.click(
    '[aria-label="Remove match bike-rental/bicycle-rented.bicycle_id=bike-42"]'
  )
  await page.waitForFunction(
    () => document.querySelectorAll("[data-matched]").length === 1
  )

  await page.click(billing)
  await page.waitForFunction(() =>
    document
      .querySelector("#studio-correlation-panel [role='alert']")
      ?.textContent.includes("Match needs the context's payloads")
  )
  assert.match(
    await text(page, "#studio-correlation-panel [role='alert']"),
    /400.*include=context:billing/s
  )
  await page.click(billing)
  await page.waitForSelector(
    '[data-matched="billing/invoice-opened.amount=12"]'
  )
  await page.click("::-p-text(Clear all)")
  await page.waitForFunction(
    () =>
      !document.querySelector("[data-matched]") &&
      document.querySelectorAll("[data-correlation-id]").length === 3
  )
  assert.equal(await page.$('[aria-label="Match 1 context"]'), null)
  assert.equal(
    await page.evaluate(() =>
      localStorage.getItem("rostfrei-tracer-studio-correlation-matches-v1")
    ),
    "{}"
  )
}

async function setValue(page, selector, value) {
  await page.$eval(selector, (input) => {
    const setter = Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value"
    ).set
    setter.call(input, "")
    input.dispatchEvent(new Event("input", { bubbles: true }))
  })
  await page.type(selector, value)
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
