import assert from "node:assert/strict"
import { tmpdir } from "node:os"
import path from "node:path"
import { parseArgs } from "node:util"
import {
  startStudioServer,
  launchStudioBrowser,
  closeStudioBrowser,
  settlePage,
  capturePage,
  observePage,
} from "./browser.mjs"
import { checkCommandRefresh } from "./command-refresh.mjs"
import { checkHandlerRefresh } from "./handler-refresh.mjs"
import { checkCommandExecution } from "./command-execution.mjs"
import { checkApiTimeouts } from "./api-timeouts.mjs"
import {
  SAMPLE_TESTS,
  SAMPLE_DEFINITIONS,
  SAMPLE_FIXTURE,
  SAMPLE_GRAPH,
  SAMPLE_FIXTURES,
} from "./fixtures.mjs"
import { checkContinuousObservation } from "./observation-smoke.mjs"
import { checkCorrelations } from "./correlation-smoke.mjs"
import { checkEvaluation } from "./evaluation-smoke.mjs"
import { installScenario } from "./visual-scenarios.mjs"

const { values } = parseArgs({ options: { check: { type: "string" } } })
if (
  values.check &&
  !["saved-runs", "navigation", "evaluation"].includes(values.check)
)
  throw new Error("--check supports saved-runs, navigation or evaluation")

const { server, url } = await startStudioServer({
  define: { "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api") },
})

// All Tracer requests are intercepted before navigation. This test never resets
// a real scenario or publishes a real command, even if a Tracer is running.
let apiMode = "offline"
const sample = {
  tests: SAMPLE_TESTS,
  definitions: SAMPLE_DEFINITIONS,
  fixture: SAMPLE_FIXTURE,
  graph: SAMPLE_GRAPH,
}
let failExpectations = false
let disconnectRun = false
const publications = []
let releaseFirstRun
const firstRunGate = new Promise((resolve) => {
  releaseFirstRun = resolve
})
let browser
let page

async function checkStudio() {
  if (values.check === "evaluation") return checkEvaluation()
  browser = await launchStudioBrowser()
  if (values.check !== "saved-runs")
    await checkWorkspaceNavigation(browser, url)
  if (values.check === "navigation") return
  await checkSavedRunInspection(browser, url)
  if (values.check === "saved-runs") return
  await checkThemeBoot(browser, url)
  await browser
    .defaultBrowserContext()
    .overridePermissions(url, ["clipboard-read", "clipboard-sanitized-write"])
  page = await browser.newPage()
  const errors = []
  page.on("pageerror", (error) => errors.push(error.message))
  await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 1 })
  await page.setRequestInterception(true)
  page.on("request", (request) => {
    void handleRequest(request)
  })
  await page.goto(url, { waitUntil: "networkidle0" })
  await page.waitForSelector('[data-connection="disconnected"]')
  assert.match(
    await text(page, ".connection-notice"),
    /GET \/api\/catalog: 502/
  )
  assert.match(await text(page, ".connection-status"), /Disconnected/)
  assert.equal(
    await page.$$eval(
      "[data-graph-node], .test-row, .run-row",
      (elements) => elements.length
    ),
    0,
    "an unavailable API must not generate demo tests, messages, or runs"
  )
  assert.equal(await page.$(".run-button"), null)
  await screenshot(page, "disconnected")
  console.log("Verified disconnected startup without demo data")

  await page.evaluate((sample) => {
    const run = {
      testId: sample.tests[0].id,
      testName: "Legacy demo",
      createdAt: new Date().toISOString(),
      status: "passed",
      nodes: sample.graph,
    }
    localStorage.setItem(
      "rostfrei-tracer-studio-runs-v1",
      JSON.stringify([
        { ...run, runId: "demo-old-format" },
        { ...run, runId: "demo-with-source", source: "demo" },
      ])
    )
  }, sample)
  await page.reload({ waitUntil: "networkidle0" })
  await page.waitForSelector('[data-connection="disconnected"]')
  assert.equal(
    await page.$$eval(".run-row", (rows) => rows.length),
    0,
    "old demo results must not be presented as recorded Tracer runs"
  )
  assert.equal(
    await page.evaluate(
      () =>
        JSON.parse(localStorage.getItem("rostfrei-tracer-studio-runs-v1"))
          .length
    ),
    2,
    "filtering old demo entries must not modify stored data during loading"
  )

  for (const [mode, reason] of [
    ["unauthorized", /401.*Authentication failed.*VITE_TRACER_TOKEN/],
    ["network-error", /Could not reach the Tracer API/],
    ["invalid-json", /invalid or incomplete JSON response/],
    ["unsupported-catalog", /Unsupported Tracer catalog version 2/],
    ["invalid-list", /did not return a test list/],
    ["fixture-error", /GET \/api\/test-scenario\/fixtures\/demo-fleet: 503/],
  ]) {
    apiMode = mode
    console.log(`Checking connection recovery: ${mode}`)
    await page.click(".connection-retry")
    await page.waitForSelector(
      `[data-connection="${["invalid-list", "fixture-error"].includes(mode) ? "tests-unavailable" : "disconnected"}"]`
    )
    assert.match(await text(page, ".connection-error-detail"), reason)
    assert.equal(
      await page.$$eval(
        "[data-graph-node], .test-row, .run-row",
        (elements) => elements.length
      ),
      0
    )
  }
  apiMode = "live"
  await page.click(".connection-retry")
  await ready(page)
  assert.equal(
    publications.length,
    0,
    "connection retries must only discover tests, never execute one"
  )
  assert.equal(await page.$(".connection-notice"), null)
  console.log("Verified read-only reconnect; checking canvas and test panel")
  await checkThemes(page)
  assert.equal(await page.$('[aria-label="Studio layout"]'), null)
  const canvasBeforeShortcut = await canvasGeometry(page)
  await page.keyboard.press("2")
  assert.deepEqual(await canvasGeometry(page), canvasBeforeShortcut)

  assert.match(await text(page, ".connection-status"), /Isolated Test/)
  assert.equal(await page.$(".studio-heading .run-button"), null)
  assert.doesNotMatch(
    await text(page, ".studio-heading"),
    /Ready to run|Showing the test definition|reset isolated Test state/
  )
  assert.ok(await page.$(".react-flow__background"))
  await page.click(".test-panel-trigger")
  await page.select("#studio-test-select", "reject-unavailable-bicycle")
  await ready(page)
  assert.match(
    await text(page, ".execution-header h1"),
    /Reject a maintenance-required bicycle/
  )
  await page.select("#studio-test-select", "rent-available-bicycle")
  await ready(page)
  await page.click('#studio-test-panel [aria-label="Close test panel"]')
  assert.equal(publications.length, 0, "choosing a test only previews its flow")
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    5
  )
  await page.click('[aria-label="Command rent-bicycle"]')
  assert.match(await text(page, "[data-command-response]"), /Expected outcome/)
  assert.doesNotMatch(
    await text(page, ".message-inspector"),
    /Response\s+status\s+accepted/
  )
  await assertFitted(page)
  await screenshot(page, "expected")

  await page.evaluate(() => {
    window.__sawPendingResponse = false
    window.__pendingObserver = new MutationObserver(() => {
      if (
        document
          .querySelector(".message-inspector")
          ?.textContent.includes("Waiting for the command response")
      )
        window.__sawPendingResponse = true
    })
    window.__pendingObserver.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true,
    })
  })
  await runSelectedTest(page)
  await page.waitForSelector('.execution-result[data-status="running"]')
  assert.equal(
    await page.$eval(".run-button", (button) => button.disabled),
    true
  )
  assert.equal(
    await page.$$eval(".test-row", (buttons) =>
      buttons.every((button) => button.disabled)
    ),
    true
  )
  releaseFirstRun()
  await passed(page)
  assert.equal(
    await page.evaluate(() => {
      window.__pendingObserver.disconnect()
      return window.__sawPendingResponse
    }),
    true,
    "a pending command must not display a previous accepted response"
  )
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    5
  )
  // The run verdict arrives before React Flow measures handles and renders edges.
  await page.waitForFunction(
    () => document.querySelectorAll("[data-graph-edge]").length === 4,
    { timeout: 5000 }
  )
  assert.match(
    await text(page, ".execution-result"),
    /Command accepted as expected/
  )
  assert.match(await text(page, ".run-button"), /Run test/)
  await assertFitted(page)
  await screenshot(page, "observed")

  const canvasBeforeDismissal = await canvasGeometry(page)
  await page.click(".react-flow__pane", { offset: { x: 12, y: 12 } })
  await page.waitForSelector(".message-inspector", { hidden: true })
  assert.deepEqual(
    await canvasGeometry(page),
    canvasBeforeDismissal,
    "clicking the canvas closes message details without moving the graph"
  )
  await assertFitted(page)
  const canvasBeforePanel = await canvasGeometry(page)
  await screenshot(page, "canvas")
  const beforePanel = publications.length
  await page.click(".test-panel-trigger")
  await page.waitForSelector("#studio-test-panel.studio-floating-panel-open")
  assert.equal(
    await page.$eval("#studio-test-select", (select) => select.value),
    "rent-available-bicycle"
  )
  await page.click(".test-definition summary")
  await page.waitForSelector(".scenario-summary", { visible: true })
  assert.match(await text(page, ".scenario-summary"), /bicycle_id: bike-42/)
  await page.keyboard.press("Escape")
  await page.waitForSelector("#studio-test-panel.studio-floating-panel-open", {
    hidden: true,
  })
  assert.deepEqual(await canvasGeometry(page), canvasBeforePanel)
  assert.equal(
    publications.length,
    beforePanel,
    "opening the test panel and its definition is read-only"
  )
  await page.click('[aria-label="Domain event bicycle-rented"]')
  await page.waitForSelector(".message-inspector", { visible: true })
  await page.waitForFunction(
    () =>
      document.querySelector(".inspector-message-heading h2")?.textContent ===
      "bicycle-rented"
  )
  await screenshot(page, "canvas-inspector")
  await page.click(".inspector-close")
  await page.waitForSelector(".message-inspector", { hidden: true })
  await page.waitForFunction(
    () =>
      document.activeElement?.getAttribute("aria-label") ===
      "Domain event bicycle-rented"
  )
  await page.keyboard.press("Enter")
  await page.waitForSelector(".message-inspector", { visible: true })
  assert.deepEqual(
    await canvasGeometry(page),
    canvasBeforePanel,
    "message inspection overlays the unchanged canvas"
  )
  await assertFitted(page)

  await page.click('[aria-label="Domain event bicycle-rented"]')
  await page.waitForFunction(
    () =>
      document.querySelector(".inspector-message-heading h2")?.textContent ===
      "bicycle-rented"
  )
  assert.equal(
    await page.$eval(
      ".inspector-message-heading h2",
      (element) => element.textContent
    ),
    "bicycle-rented"
  )
  assert.match(
    await text(page, ".relationship-description"),
    /Caused by rent-bicycle/
  )
  await page.click('[data-copy-identity="message ID"]')
  await page.waitForFunction(
    () =>
      document.querySelector('[data-copy-identity="message ID"]')
        ?.textContent === "Copied"
  )
  assert.equal(
    await page.evaluate(() => navigator.clipboard.readText()),
    "mock-1-event-rented"
  )
  await page.mouse.move(5, 5)
  assert.equal(
    await page.$eval(
      ".inspector-message-heading h2",
      (element) => element.textContent
    ),
    "bicycle-rented",
    "details must persist after pointer movement"
  )
  assert.equal(await page.$("[data-node-popup]"), null)

  const edgeGeometry = await page.evaluate(() =>
    [...document.querySelectorAll("[data-graph-edge]")].map((edge) => {
      const line = edge.querySelector(":scope > path")
      const marker = edge.querySelector("marker")
      const source = document
        .querySelector(
          `[data-node-id="${edge.dataset.sourceId}"] .message-card`
        )
        .getBoundingClientRect()
      const target = document
        .querySelector(
          `[data-node-id="${edge.dataset.targetId}"] .message-card`
        )
        .getBoundingClientRect()
      const start = line
        .getPointAtLength(0)
        .matrixTransform(line.getScreenCTM())
      const end = line
        .getPointAtLength(line.getTotalLength())
        .matrixTransform(line.getScreenCTM())
      return {
        causal: edge.dataset.edgeRelationship === "causation",
        marker: marker?.id,
        markerEnd: line.getAttribute("marker-end"),
        error: Math.max(
          Math.abs(start.x - source.right),
          Math.abs(end.x - target.left),
          Math.abs(start.y - (source.top + source.height / 2)),
          Math.abs(end.y - (target.top + target.height / 2))
        ),
      }
    })
  )
  for (const edge of edgeGeometry) {
    assert.ok(
      edge.error <= 2,
      `edge must meet the visible card boundary: ${JSON.stringify(edge)}`
    )
    assert.equal(
      Boolean(edge.marker),
      edge.causal,
      "only causal edges have arrows"
    )
    if (edge.causal) assert.equal(edge.markerEnd, `url(#${edge.marker})`)
  }

  const zoomBefore = Number(
    (await text(page, ".graph-zoom-value")).replace("%", "")
  )
  await page.click('button[aria-label="Zoom in"]')
  await page.waitForFunction(
    (previous) =>
      Number(
        document.querySelector(".graph-zoom-value").textContent.replace("%", "")
      ) > previous,
    {},
    zoomBefore
  )
  const transformBefore = await page.$eval(
    ".react-flow__viewport",
    (element) => element.style.transform
  )
  const pane = await page.$(".react-flow__pane")
  const bounds = await pane.boundingBox()
  await page.mouse.move(bounds.x + 12, bounds.y + bounds.height - 12)
  await page.mouse.down()
  await page.mouse.move(bounds.x + 62, bounds.y + bounds.height - 52, {
    steps: 5,
  })
  await page.mouse.up()
  await page.waitForFunction(
    (previous) =>
      document.querySelector(".react-flow__viewport").style.transform !==
      previous,
    {},
    transformBefore
  )
  assert.ok(
    await page.$eval(".message-inspector", (element) =>
      element.checkVisibility()
    ),
    "panning the canvas keeps message details open"
  )
  await page.click('button[aria-label="Fit graph to view"]')
  await assertFitted(page)
  assert.equal(
    await text(page, ".inspector-message-heading h2"),
    "bicycle-rented",
    "viewport controls preserve message selection"
  )
  await page.focus('[data-message-id="fixture-event-0-demo-bike-42-added"]')
  await page.keyboard.press("Enter")
  await page.waitForFunction(
    () =>
      document.querySelector(".inspector-message-heading h2").textContent ===
      "bicycle-added"
  )
  assert.match(
    await text(page, ".relationship-description"),
    /Given domain event.*stream version 1/s
  )

  await selectTest(page, "Reject a maintenance-required bicycle")
  assert.match(
    await text(page, "[data-command-response]"),
    /Expected outcome.*rejected/s
  )
  await runSelectedTest(page)
  await passed(page)
  assert.match(
    await text(page, ".execution-result"),
    /Test passed.*Command rejected as expected/s
  )
  assert.match(
    await text(page, "[data-command-response]"),
    /BICYCLE_UNAVAILABLE/
  )
  assert.match(await text(page, ".message-inspector"), /bike-99/)
  await screenshot(page, "rejection")
  await runSelectedTest(page)
  await page.waitForFunction(
    () => document.querySelectorAll(".run-row").length === 3
  )
  await passed(page)
  assert.match(await text(page, ".run-button"), /Run test/)

  // Reloaded history retains the definition and outcome returned by Tracer.
  await page.reload({ waitUntil: "networkidle0" })
  await ready(page)
  await selectRun(page, "Reject a maintenance-required bicycle")
  assert.match(
    await text(page, ".execution-result"),
    /Command rejected as expected/
  )
  assert.match(
    await text(page, ".connection-status"),
    /Isolated Test.*Saved run/
  )

  for (const width of [1440, 1280]) {
    await page.setViewport({ width, height: 800, deviceScaleFactor: 1 })
    await page.reload({ waitUntil: "networkidle0" })
    await ready(page)
    await selectRun(page, "Rent an available bicycle")
    await assertFitted(page)
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      ),
      true
    )
    await screenshot(page, `desktop-${width}`)
  }

  for (const width of [1024, 768, 390, 320]) {
    await page.setViewport({ width, height: 844, deviceScaleFactor: 1 })
    await page.reload({ waitUntil: "networkidle0" })
    await ready(page)
    await page.waitForSelector(".message-list", { visible: true })
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      ),
      true
    )
    await page.click('.message-list button[aria-label="Command rent-bicycle"]')
    await page.waitForFunction(() =>
      document.activeElement?.classList.contains("message-inspector")
    )
    assert.match(
      await text(page, "[data-command-response]"),
      /Expected outcome/
    )
    assert.equal(
      await page.$eval(
        ".message-inspector",
        (element) => element.scrollWidth <= element.clientWidth
      ),
      true
    )
    if (width === 390) await screenshot(page, "mobile-inspector")
    await page.click(".inspector-back")
    await page.waitForFunction(
      () =>
        document.activeElement?.getAttribute("aria-label") ===
        "Command rent-bicycle"
    )
    if (width === 390) await screenshot(page, "mobile-flow")
  }

  await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 1 })
  await page.reload({ waitUntil: "networkidle0" })
  await ready(page)
  assert.match(await text(page, ".connection-status"), /Isolated Test/)
  await selectRun(page, "Rent an available bicycle")
  assert.match(await text(page, ".connection-status"), /Isolated Test/)
  assert.match(
    await text(page, ".saved-run-rerun"),
    /Run again/,
    "saved Tracer runs can be rerun only through the connected API"
  )
  await runSelectedTest(page)
  await passed(page)
  assert.match(await text(page, ".connection-status"), /Isolated Test/)
  assert.equal(publications.at(-1), "rent-available-bicycle")

  await selectTest(page, "Reject a maintenance-required bicycle")
  await runSelectedTest(page)
  await passed(page)
  await selectTest(page, "Return a rented bicycle")
  await selectRun(page, "Reject a maintenance-required bicycle")
  await runSelectedTest(page)
  await passed(page)
  assert.equal(
    publications.at(-1),
    "reject-unavailable-bicycle",
    "rerunning history must execute its own test, not the previously viewed definition"
  )
  assert.match(await text(page, ".message-inspector"), /bike-99/)

  failExpectations = true
  await selectTest(page, "Rent an available bicycle")
  await runSelectedTest(page)
  await page.waitForSelector('.execution-result[data-status="failed"]')
  assert.match(
    await text(page, ".execution-result"),
    /Command accepted.*expectations were not met/s
  )
  await page.click(".run-diagnostics summary")
  assert.match(
    await text(page, ".run-diagnostics"),
    /Expected bicycle-rented was not observed/
  )
  await screenshot(page, "failed-expectation")
  failExpectations = false

  disconnectRun = true
  const previousPublications = publications.length
  await runSelectedTest(page)
  await page.waitForSelector('.execution-result[data-status="unavailable"]')
  assert.match(await text(page, ".execution-result"), /No complete run report/)
  assert.equal(
    publications.length,
    previousPublications + 1,
    "ambiguous submissions must not retry automatically"
  )
  disconnectRun = false

  await page.evaluate(() => {
    window.__originalSetItem = Storage.prototype.setItem
    Storage.prototype.setItem = () => {
      throw new DOMException("Storage unavailable", "QuotaExceededError")
    }
  })
  await runSelectedTest(page)
  await passed(page)
  assert.match(
    await text(page, ".sidebar-footer"),
    /Browser storage could not save/
  )
  assert.match(await text(page, ".execution-result"), /Test passed/)
  await page.evaluate(() => {
    Storage.prototype.setItem = window.__originalSetItem
  })

  const semantics = await page.evaluate(async (sample) => {
    const { expectedGraph, reportGraph, layoutMessageGraph } =
      await import("/src/lib/graph.ts")
    const { messageFacts } = await import("/src/lib/message-presentation.ts")
    const fixture = structuredClone(sample.fixture)
    fixture.messages = [fixture.messages[0]]
    fixture.messages.push({
      ...fixture.messages[0],
      messageId: "fixture-second",
      streamVersion: 2,
    })
    fixture.messages.push({
      ...fixture.messages[0],
      messageId: "other-stream",
      aggregate: { ...fixture.messages[0].aggregate, id: "other-fleet" },
    })
    const definition = sample.definitions["rent-available-bicycle"].definition
    const expected = layoutMessageGraph(expectedGraph(definition, fixture))
    const report = {
      expected: definition.expected,
      observed: {
        messages: [
          {
            kind: "command",
            name: "rent-bicycle",
            messageId: "command",
            observationOrder: 1,
            schemaVersion: 1,
            context: "bike-rental",
          },
          {
            kind: "domain-event",
            name: "unlinked-event",
            messageId: "event",
            observationOrder: 2,
            schemaVersion: 1,
          },
        ],
        commandOutcomes: [],
      },
      comparison: {
        matches: [{ expectedKey: "subject", observedMessageId: "command" }],
      },
      operation: { status: "completed" },
    }
    const observed = layoutMessageGraph(reportGraph(report, fixture))
    return {
      streamEdges: expected.edges
        .filter((edge) => edge.relationship === "stream-order")
        .map((edge) => [edge.source.messageId, edge.target.messageId]),
      contextEdges: expected.edges.filter(
        (edge) => edge.relationship === "context"
      ).length,
      unresolvedEdges: observed.edges.filter(
        (edge) => edge.target.id === "event"
      ).length,
      missingOutcome:
        observed.nodes.find((node) => node.id === "command").status ?? null,
      scalarFacts: [null, false, 0, ""].map((payload) =>
        messageFacts({ payload })
      ),
    }
  }, sample)
  assert.deepEqual(semantics.streamEdges, [
    ["demo-bike-42-added", "fixture-second"],
  ])
  assert.equal(semantics.contextEdges, 1)
  assert.equal(
    semantics.unresolvedEdges,
    0,
    "observation order must not invent a causal arrow"
  )
  assert.equal(
    semantics.missingOutcome,
    "indeterminate",
    "a completed operation alone does not prove command acceptance"
  )
  assert.deepEqual(semantics.scalarFacts, [
    [["Value", "null"]],
    [["Value", "false"]],
    [["Value", "0"]],
    [["Value", '""']],
  ])

  for (const width of [1280, 390]) {
    await page.setViewport({ width, height: 844, deviceScaleFactor: 1 })
    await page.reload({ waitUntil: "networkidle0" })
    await ready(page)
    assert.equal(await page.$('[aria-label="Studio layout"]'), null)
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      ),
      true
    )
    await page.waitForSelector(".message-inspector", { hidden: true })
    if (width === 1280) await assertFitted(page)
    await screenshot(page, `canvas-${width}`)
    await page.click('button[aria-label="Command rent-bicycle"]')
    await page.waitForSelector(".message-inspector", { visible: true })
    if (width === 390) {
      await page.click(".inspector-back")
      await page.waitForSelector(".message-list", { visible: true })
    }
  }

  apiMode = "offline"
  await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 1 })
  await page.reload({ waitUntil: "networkidle0" })
  await page.waitForSelector('[data-connection="disconnected"]')
  await page.click('[data-workspace-tab="runs"]')
  if (await page.$('button[aria-label="Expand sidebar"]'))
    await page.click('button[aria-label="Expand sidebar"]')
  await page.evaluate(() => document.querySelector(".run-row").click())
  await page.waitForSelector(".execution-result")
  assert.match(
    await text(page, ".connection-status"),
    /Disconnected.*Saved run/
  )
  assert.equal(
    await page.$eval(".saved-run-rerun", (button) => button.disabled),
    true,
    "saved history must not enable offline execution"
  )
  assert.match(await text(page, ".execution-note"), /Viewing a saved run/)
  const beforeReconnect = publications.length
  apiMode = "empty"
  await page.click(".connection-retry")
  await page.waitForSelector('.connection-status[data-source="live"]')
  await page.click('[data-workspace-tab="tests"]')
  await page.click(".test-panel-trigger")
  assert.match(
    await text(page, "#studio-test-panel"),
    /No behavioral tests are registered/
  )
  assert.equal(await page.$("#studio-test-panel .run-button"), null)
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    0
  )
  assert.equal(publications.length, beforeReconnect)
  assert.deepEqual(errors, [], "no browser runtime errors")
  await checkCommandExecution(browser, url)
  await checkCommandRefresh(browser, url)
  await checkHandlerRefresh(browser, url)
  await checkApiTimeouts(browser, url)
  console.log("Verified API request deadlines and partial capture evidence")
  await checkContinuousObservation()
  await checkCorrelations()
  await checkEvaluation()
  console.log(
    "Verified the optional Evaluation mode: runs, case matrix, regression, sample verdicts and correlation links"
  )
  console.log(
    `PASS: explicit connection failures and read-only retry; no demo fallback; canonical bicycle-added fixtures; test panel, graph-first runs and offline history; overlay inspector and causal edges; 6 responsive widths; Test-default command forms, explicit uncommitted handler diagnostics without graph/capture, retired Preview absence, exact numeric payloads, idempotency, input refresh and operation identities; failed expectations, ambiguous responses and storage failure. All API responses were mocked. Screenshots: ${path.join(tmpdir(), "rostfrei-studio-*.png")}`
  )
}

try {
  await checkStudio()
} catch (error) {
  console.error(error)
  if (page && !page.isClosed()) {
    try {
      await screenshot(page, "failure")
      console.error(await text(page, ".studio-heading"))
    } catch {
      // Preserve the original error if Chrome is no longer available.
    }
  }
  throw error
} finally {
  console.log("Closing UI browser")
  await closeStudioBrowser(browser)
  console.log("Closing UI server")
  await server.close()
  console.log("UI resources closed")
}

async function checkWorkspaceNavigation(browser, url) {
  const context = await browser.createBrowserContext()
  try {
    const page = await context.newPage()
    await page.setViewport({ width: 1440, height: 900 })
    const diagnostics = observePage(page)
    const writes = []
    page.on("request", (request) => {
      if (request.method() !== "GET") writes.push(request.method())
    })
    await installScenario(
      page,
      "overview",
      { SAMPLE_TESTS, SAMPLE_DEFINITIONS, SAMPLE_FIXTURES },
      diagnostics
    )
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector(".run-button:not(:disabled)")
    const modes = ["commands", "tests", "runs", "observation", "correlations"]
    assert.deepEqual(
      await page.$$eval('[role="tab"]', (tabs) =>
        tabs.map((tab) => tab.dataset.workspaceTab)
      ),
      modes,
      "Studio has five stable mode tabs"
    )
    const testNodes = await page.$$eval("[data-graph-node]", (nodes) =>
      nodes.map((node) => node.dataset.nodeId)
    )
    for (const width of [1440, 390]) {
      await page.setViewport({ width, height: 900 })
      await settlePage(page)
      const tabs = () =>
        page.$$eval('[role="tab"]', (tabs) =>
          tabs.map((tab) => ({
            label: tab.getAttribute("aria-label"),
            bounds: tab.getBoundingClientRect().toJSON(),
          }))
        )
      const original = await tabs()
      for (const [index, mode] of modes.entries()) {
        await page.keyboard.down("Control")
        await page.keyboard.press(String(index + 1))
        await page.keyboard.up("Control")
        await page.waitForSelector(`[data-workspace="${mode}"]`)
        await settlePage(page)
        assert.deepEqual(
          await tabs(),
          original,
          "mode switches never rename or move tabs"
        )
        assert.equal(
          await page.$eval(
            '[role="tab"][aria-selected="true"]',
            (tab) => tab.dataset.workspaceTab
          ),
          mode
        )
        assert.equal(
          await page.$$eval(
            '[role="tab"][tabindex="0"]',
            (tabs) => tabs.length
          ),
          1
        )
        assert.equal(
          await page.$$eval(
            ".studio-topbar .command-panel-trigger, .studio-topbar .test-panel-trigger",
            (buttons) => buttons.length
          ),
          0
        )
        if (mode === "commands") {
          assert.equal(
            await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
            0,
            "Commands does not borrow the Tests graph"
          )
          assert.ok(await page.$("#studio-workspace .command-panel-trigger"))
        }
        if (mode === "tests" && width === 1440)
          assert.deepEqual(
            await page.$$eval("[data-graph-node]", (nodes) =>
              nodes.map((node) => node.dataset.nodeId)
            ),
            testNodes
          )
      }
      assert.ok(
        original.every(
          (tab) => tab.bounds.left >= 0 && tab.bounds.right <= width
        )
      )
    }
    await page.focus('[data-workspace-tab="commands"]')
    await page.keyboard.press("ArrowRight")
    await page.waitForSelector('[data-workspace="tests"]')
    await page.keyboard.press("End")
    await page.waitForSelector('[data-workspace="correlations"]')
    await page.keyboard.press("Home")
    await page.waitForSelector('[data-workspace="commands"]')
    assert.deepEqual(writes, [], "switching tabs is read-only")
    assert.deepEqual(diagnostics.pageErrors, [])
    assert.deepEqual(diagnostics.unexpectedRequests, [])
    console.log(
      "Verified fixed mode tabs, Ctrl+1–5, tab keyboard navigation, local actions and separate graphs"
    )
  } finally {
    await context.close()
  }
}

async function checkThemeBoot(browser, url) {
  const context = await browser.createBrowserContext()
  try {
    const bootPage = await context.newPage()
    await bootPage.emulateMediaFeatures([
      { name: "prefers-color-scheme", value: "light" },
    ])
    await bootPage.evaluateOnNewDocument(() =>
      localStorage.setItem("rostfrei-theme", "dark")
    )
    await bootPage.setRequestInterception(true)
    bootPage.on("request", (request) => {
      // The first-paint theme must not depend on React or any module loading.
      if (request.resourceType() === "script") {
        void request.respond({
          status: 200,
          contentType: "text/javascript",
          body: "",
        })
      } else {
        void request.continue()
      }
    })
    await bootPage.goto(url, { waitUntil: "domcontentloaded" })
    assert.equal(
      await bootPage.$eval("html", (root) => root.className),
      "dark",
      "stored dark theme must apply before the app loads"
    )
    assert.equal(await bootPage.$(".studio-shell"), null)
  } finally {
    await context.close()
  }
}

async function checkThemes(page) {
  await settlePage(page)
  const graphTransform = await page.$eval(
    ".react-flow__viewport",
    (element) => element.style.transform
  )
  const chooseTheme = async (label) => {
    await page.click(".studio-theme-toggle")
    await page.locator(`::-p-aria(${label})`).click()
  }
  const assertTheme = async (theme) => {
    await page.waitForSelector(`html.${theme} .react-flow.${theme}`)
    await settlePage(page)
    const colors = await page.evaluate(() => ({
      background: getComputedStyle(document.body).backgroundColor,
      canvas: getComputedStyle(document.querySelector(".message-flow"))
        .backgroundColor,
      foreground: getComputedStyle(document.body).color,
      node: getComputedStyle(document.querySelector(".message-card-name"))
        .color,
    }))
    assert.equal(
      colors.canvas,
      colors.background,
      `${theme} canvas must inherit the app theme`
    )
    assert.equal(
      colors.node,
      colors.foreground,
      `${theme} nodes must inherit the app theme`
    )
  }

  await chooseTheme("Dark")
  await assertTheme("dark")
  assert.equal(
    await page.evaluate(() => localStorage.getItem("rostfrei-theme")),
    "dark"
  )
  await chooseTheme("Light")
  await assertTheme("light")
  assert.equal(
    await page.$eval(
      ".react-flow__viewport",
      (element) => element.style.transform
    ),
    graphTransform,
    "theme changes must preserve graph viewport state"
  )
  await page.reload({ waitUntil: "networkidle0" })
  await ready(page)
  await assertTheme("light")
  assert.equal(
    await page.$eval(".studio-theme-toggle", (button) =>
      button.getAttribute("aria-label")
    ),
    "Theme: Light"
  )
  await chooseTheme("System")
  for (const value of ["dark", "light"]) {
    await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value }])
    await assertTheme(value)
  }

  await page.setViewport({ width: 390, height: 844, deviceScaleFactor: 1 })
  await settlePage(page)
  assert.equal(
    await page.$$eval(".studio-topbar button", (buttons) =>
      buttons.every((button) => {
        const rect = button.getBoundingClientRect()
        return rect.left >= 0 && rect.right <= innerWidth
      })
    ),
    true,
    "every mobile header control must remain visible"
  )
  await page.click('[data-workspace-tab="commands"]')
  await page.click(".command-panel-trigger")
  await page.waitForSelector('#studio-command-panel[aria-hidden="false"]')
  // Panel focus raises its inline layer; even a long session must not cover a portal.
  await page.$eval("#studio-command-panel", (panel) => {
    panel.style.zIndex = "10000"
  })
  await page.click(".studio-theme-toggle")
  await page.waitForSelector('[role="menu"]')
  assert.equal(
    await page.$eval('[role="menu"]', (menu) => {
      const rect = menu.getBoundingClientRect()
      return (
        rect.left >= 0 && rect.right <= innerWidth && rect.bottom <= innerHeight
      )
    }),
    true,
    "mobile theme picker must stay within the viewport"
  )
  assert.ok(
    await page.$$eval('[role="menuitemradio"]', (items) =>
      items.every((item) => {
        const rect = item.getBoundingClientRect()
        return item.contains(
          document.elementFromPoint(
            rect.x + rect.width / 2,
            rect.y + rect.height / 2
          )
        )
      })
    ),
    "mobile theme choices must be above the open command panel"
  )
  await page.locator("::-p-aria(Dark)").click()
  await page.waitForSelector("html.dark")
  await page.click(".command-panel-trigger")
  await page.waitForSelector('#studio-command-panel[aria-hidden="true"]')
  await chooseTheme("System")
  await page.keyboard.press("Escape")
  await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 1 })
  await page.click('[data-workspace-tab="tests"]')
  await settlePage(page)
  console.log(
    "Verified shared light/dark/system themes, persistence, graph colors, and mobile picker"
  )
}

async function handleRequest(request) {
  const pathname = new URL(request.url()).pathname
  if (!pathname.startsWith("/api/")) return request.continue()
  const respond = (body, status = 200) =>
    request.respond({
      status,
      contentType: "application/json",
      body: JSON.stringify(body),
    })
  if (apiMode === "offline") return respond({}, 502)
  if (apiMode === "unauthorized") return respond({}, 401)
  if (apiMode === "network-error") return request.abort("failed")
  if (apiMode === "invalid-json")
    return request.respond({
      status: 200,
      contentType: "application/json",
      body: "not JSON",
    })
  if (pathname === "/api/catalog")
    return respond({
      catalogVersion: apiMode === "unsupported-catalog" ? 2 : 1,
      testRepository: { definitionsHref: "/behavior-tests" },
      contexts: [],
    })
  if (pathname === "/api/behavior-tests")
    return respond({
      items:
        apiMode === "invalid-list"
          ? null
          : apiMode === "empty"
            ? []
            : sample.tests,
    })
  if (pathname.startsWith("/api/test-scenario/fixtures/"))
    return apiMode === "fixture-error"
      ? respond({ message: "Fixture discovery unavailable" }, 503)
      : respond(SAMPLE_FIXTURES[pathname.split("/").at(-1)])
  const match = /^\/api\/tests\/([^/]+)(\/runs)?$/.exec(pathname)
  if (match && sample.definitions[match[1]]) {
    if (!match[2]) return respond(sample.definitions[match[1]])
    assert.equal(request.method(), "POST")
    publications.push(match[1])
    if (disconnectRun) return request.abort("failed")
    if (publications.length === 1) await firstRunGate
    return respond(reportFor(match[1]))
  }
  return respond({ message: "Unexpected test endpoint" }, 404)
}

async function checkSavedRunInspection(browser, url) {
  const { reportGraph } = await server.ssrLoadModule("/src/lib/graph.ts")
  const runs = ["rent-available-bicycle", "reject-unavailable-bicycle"].map(
    (testId) => {
      const report = reportFor(testId)
      const definition = sample.definitions[testId]
      return {
        runId: `saved-${testId}`,
        testId,
        testName: definition.definition.name,
        status: report.status,
        createdAt: "2026-11-08T12:00:00.000Z",
        source: "live",
        nodes: reportGraph(
          report,
          SAMPLE_FIXTURES[definition.definition.setup.fixture]
        ),
        definition,
        diagnostics: report.comparison.diagnostics,
      }
    }
  )
  const disconnected = structuredClone(runs[0])
  runs[1].capture = { fidelity: "exact", settled: "true" }
  disconnected.runId = "saved-disconnected"
  disconnected.testName = "Saved flow with an absent cause"
  const orphan = disconnected.nodes.at(-1)
  orphan.parentId = "not-in-this-capture"
  orphan.hideIncomingEdge = true
  orphan.edgeFidelity = "grouped"
  disconnected.capture = {
    fidelity: "grouped",
    settled: false,
    settledFor: "500ms",
    note: "Capture window expired",
  }
  disconnected.diagnostics = [
    {
      code: "MISSING_PARENT",
      path: "/messages/2",
      message: "Cause was not captured",
    },
  ]
  runs.push(disconnected)
  for (const width of [1440, 390]) {
    const context = await browser.createBrowserContext()
    const page = await context.newPage()
    const writes = []
    const diagnostics = observePage(page)
    const selectSavedRun = async (run) => {
      const textSelector = `::-p-text(${run.testName})`
      const selector = `#studio-navigation:not([inert]) .run-row ${textSelector}`
      try {
        await page.waitForSelector('[data-workspace-tab="runs"]')
        await page.click('[data-workspace-tab="runs"]')
        const toggle = 'button[aria-controls="studio-navigation"]'
        // Network idle can precede React's first render; wait before reading open state.
        await page.waitForSelector(toggle, { visible: true })
        if (
          await page.$eval(
            toggle,
            (button) => button.getAttribute("aria-expanded") === "false"
          )
        )
          await page.locator(toggle).click()
        await page
          .locator(selector)
          .map((element) => element.closest("button.run-row"))
          .click()
        await page.waitForFunction(
          (name) =>
            document.querySelector('.run-row[aria-current="page"] .run-name')
              ?.textContent === name,
          {},
          run.testName
        )
      } catch (error) {
        try {
          const matches = await page.$$eval(textSelector, (elements) =>
            elements.map((element) => {
              const rect = element.getBoundingClientRect()
              const top = document.elementFromPoint(
                rect.x + rect.width / 2,
                rect.y + rect.height / 2
              )
              return {
                html: element.outerHTML,
                bounds: rect.toJSON(),
                visible: element.checkVisibility({ visibilityProperty: true }),
                inert: Boolean(element.closest("[inert]")),
                disabled: element.closest("button")?.disabled,
                hit: top?.outerHTML,
              }
            })
          )
          const navigation = await page.evaluate(() => {
            const sidebar = document.querySelector(".studio-sidebar")
            return {
              classes: sidebar?.className,
              inert: sidebar?.inert,
              bounds: sidebar?.getBoundingClientRect().toJSON(),
              button: document.querySelector(
                '[aria-controls="studio-navigation"]'
              )?.outerHTML,
              rows: [...(sidebar?.querySelectorAll(".run-row") ?? [])].map(
                (row) => row.outerHTML
              ),
            }
          })
          console.error(
            await capturePage(
              page,
              path.join("artifacts/review", `saved-run-inspection-${width}`),
              {
                scene: "saved-run-inspection",
                source: "mocked",
                diagnostics,
                selector,
                selectionError: error.stack,
                matches,
                navigation,
              }
            )
          )
        } catch (captureError) {
          console.error(
            "Could not capture saved-run selection failure",
            captureError
          )
        }
        throw error
      }
    }
    await page.setViewport({ width, height: 900 })
    await page.evaluateOnNewDocument((runs) => {
      localStorage.setItem("rostfrei-tracer-studio-layout-v1", "workbench")
      localStorage.setItem(
        "rostfrei-tracer-studio-runs-v1",
        JSON.stringify(runs)
      )
    }, runs)
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      const pathname = new URL(request.url()).pathname
      if (!pathname.startsWith("/api/")) return void request.continue()
      if (request.method() !== "GET") writes.push(pathname)
      const body =
        pathname === "/api/catalog"
          ? { catalogVersion: 1, contexts: [] }
          : { message: "Unexpected saved-inspection request" }
      void request.respond({
        status: pathname === "/api/catalog" ? 200 : 404,
        contentType: "application/json",
        body: JSON.stringify(body),
      })
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    for (const [index, outcome] of ["accepted", "rejected"].entries()) {
      await selectSavedRun(runs[index])
      await settlePage(page)
      assert.equal(await page.$('[aria-label="Studio layout"]'), null)
      assert.ok(
        await page.$(".message-graph"),
        "saved runs open directly on the canvas, including browsers with an old layout preference"
      )
      assert.equal(await page.$(".run-inspection"), null)
      if (width > 1100)
        assert.ok(
          await page.$(".react-flow__background"),
          "the canvas has a visible dot grid"
        )
      await page.locator("button::-p-text(Run summary)").click()
      await page.waitForSelector(".run-inspection")
      assert.match(
        await text(page, ".run-inspection .rf-evidence-facts"),
        /Test.*Passed/s
      )
      assert.match(
        await text(page, ".run-inspection .rf-evidence-facts"),
        /Capture.*unavailable/is
      )
      assert.match(
        await text(page, ".rf-evidence-summary"),
        /No capture-window or settling metadata/
      )
      assert.match(
        await text(page, ".run-inspection"),
        new RegExp(`Command.*${outcome}`, "is")
      )
      assert.equal(
        await page.$$eval("[data-evidence-message]", (rows) => rows.length),
        index ? 1 : 3,
        "the first flow contains observed messages, with fixture context disclosed separately"
      )
      assert.equal(
        await page.$$eval("[data-evidence-edge]", (edges) => edges.length),
        index ? 0 : 2
      )
      assert.doesNotMatch(
        await page.$eval(".run-inspection", (root) => root.innerText),
        /bike-(42|99)|mock-0/,
        "initial inspection shows neither payload fields nor technical identities"
      )
      assert.equal(
        await page.$eval(".run-inspection", (root) =>
          [...root.querySelectorAll("details")].some((item) => item.open)
        ),
        false
      )
      assert.equal(
        await page.$(".run-inspection .message-inspector"),
        null,
        "message details are opened on demand"
      )
      await page.focus("[data-evidence-message] button")
      await page.keyboard.press("Enter")
      await page.waitForSelector(".run-inspection .message-inspector", {
        visible: true,
      })
      assert.match(
        await text(page, ".run-inspection [data-command-response]"),
        index ? /BICYCLE_UNAVAILABLE/ : /accepted/
      )
      const payload = await page.$(
        ".run-inspection [data-inspection-payload] details"
      )
      assert.ok(payload, "payload has an explicit disclosure")
      assert.equal(await payload.evaluate((element) => element.open), false)
      await page.click(".run-inspection [data-inspection-payload] summary")
      assert.equal(await payload.evaluate((element) => element.open), true)
      await page.click(".run-inspection .inspector-back")
      await page.waitForFunction(() =>
        document.activeElement?.closest("[data-evidence-message]")
      )
      assert.deepEqual(
        writes,
        [],
        "inspection and disclosure never execute or reset a test"
      )
      assert.equal(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth
        ),
        true
      )
      await screenshot(page, `saved-${outcome}-${width}`)
    }
    await selectSavedRun(disconnected)
    await settlePage(page)
    await page.locator("button::-p-text(Run summary)").click()
    await page.waitForSelector(".run-inspection")
    assert.equal(
      await page.$$eval("[data-evidence-message]", (rows) => rows.length),
      3
    )
    assert.equal(
      await page.$$eval("[data-evidence-edge]", (edges) => edges.length),
      1,
      "an absent cause does not gain an arrow from observation order"
    )
    assert.match(
      await text(page, ".run-inspection"),
      /No resolved causal parent/
    )
    assert.match(
      await page.$eval(".run-inspection", (root) => root.innerText),
      /Capture.*Grouped, not settled/is
    )
    assert.match(
      await page.$eval(".run-inspection", (root) => root.innerText),
      /MISSING_PARENT/
    )
    assert.match(
      await text(page, ".rf-evidence-summary"),
      /1 message has no resolved causal parent/
    )
    assert.equal(
      await page.$eval(".run-inspection", (root) =>
        [...root.querySelectorAll("details")].some((item) => item.open)
      ),
      false
    )
    assert.deepEqual(writes, [])
    await context.close()
  }
  console.log(
    "Verified graph-first saved runs, optional summary, progressive details, keyboard return and read-only inspection"
  )
}

function reportFor(testId) {
  const definition = sample.definitions[testId].definition
  const expectedNodes = definition.expected.graphs[0].nodes
  const root = expectedNodes.find(
    (node) => node.kind === "command" && !node.parentKey
  )
  const accepted = root.outcome === "accepted"
  const identity = `mock-${publications.length}`
  const observedNodes =
    !failExpectations && testId === "rent-available-bicycle"
      ? sample.graph
          .filter((node) => !node.context)
          .map((node) => ({
            ...node,
            key: node.id,
            parentKey:
              node.edgeRelationship === "context" ? undefined : node.parentId,
            context: node.boundedContext,
          }))
      : failExpectations
        ? [root]
        : expectedNodes
  const messages = observedNodes.map((node, index) => ({
    kind: node.kind,
    name: node.name,
    schemaVersion: node.schemaVersion,
    payload: node.payload,
    context: node.context,
    messageId: `${identity}-${node.key}`,
    causationId: node.parentKey ? `${identity}-${node.parentKey}` : undefined,
    correlationId: identity,
    observationOrder: index + 1,
  }))
  const commandOutcome = {
    responseMessageId: `${identity}-response`,
    commandMessageId: messages.find((message) => message.kind === "command")
      .messageId,
    correlationId: identity,
    observationOrder: 2,
    outcome: accepted
      ? { status: "accepted", value: null }
      : {
          status: "rejected",
          value: {
            classification: "conflict",
            code: root.outcome.rejected.code,
            message: "The requested bicycle cannot currently be rented.",
            details: root.outcome.rejected.payload,
          },
        },
  }
  const status = failExpectations ? "failed" : "passed"
  return {
    runId: identity,
    testId,
    status,
    expected: definition.expected,
    observed: { messages, commandOutcomes: [commandOutcome] },
    commandOutcome,
    comparison: {
      status,
      matches: expectedNodes.flatMap((node) => {
        const message = messages.find((message) => message.name === node.name)
        return message
          ? [{ expectedKey: node.key, observedMessageId: message.messageId }]
          : []
      }),
      diagnostics: failExpectations
        ? [
            {
              code: "missing-message",
              path: "/expected/graphs/0/nodes/1",
              message: "Expected bicycle-rented was not observed.",
            },
          ]
        : [],
    },
    operation: {
      operationId: identity,
      status: "completed",
      result: { decision: accepted ? "accepted" : "rejected" },
    },
  }
}

async function selectTest(page, name) {
  await page.click('[data-workspace-tab="tests"]')
  if (await page.$('button[aria-label="Expand sidebar"]'))
    await page.click('button[aria-label="Expand sidebar"]')
  await page.evaluate(
    (name) =>
      [...document.querySelectorAll(".test-row")]
        .find((button) => button.textContent === name)
        .click(),
    name
  )
  await ready(page)
  if (page.viewport().width > 1100)
    await page.click('.message-card[aria-label^="Command "]')
}

async function selectRun(page, name) {
  await page.click('[data-workspace-tab="runs"]')
  if (await page.$('button[aria-label="Expand sidebar"]'))
    await page.click('button[aria-label="Expand sidebar"]')
  await page.evaluate(
    (name) =>
      [...document.querySelectorAll(".run-row")]
        .find(
          (button) => button.querySelector(".run-name").textContent === name
        )
        .click(),
    name
  )
  await passed(page)
}

async function ready(page) {
  await page.waitForFunction(
    () =>
      document.querySelector(".run-button") &&
      !document.querySelector(".run-button").disabled
  )
}

async function passed(page) {
  await page.waitForSelector('.execution-result[data-status="passed"]', {
    timeout: 15000,
  })
  assert.equal(
    await page.$(".run-inspection"),
    null,
    "completed tests stay on the canvas"
  )
  await ready(page)
  if (page.viewport().width > 1100)
    await page.click('.message-card[aria-label^="Command "]')
}

async function runSelectedTest(page) {
  if (await page.$('[data-workspace="runs"]'))
    await page.click(".saved-run-rerun")
  await page.click('[data-workspace-tab="tests"]')
  if (await page.$('.test-panel-trigger[aria-expanded="false"]'))
    await page.click(".test-panel-trigger")
  await page.waitForSelector("#studio-test-panel.studio-floating-panel-open")
  await page.click("#studio-test-panel .run-button")
}

async function canvasGeometry(page) {
  return page.$eval(".message-graph", (graph) => ({
    bounds: graph.getBoundingClientRect().toJSON(),
    transform: graph.querySelector(".react-flow__viewport")?.style.transform,
  }))
}

async function assertFitted(page) {
  await page.waitForFunction(() => {
    const bounds = document
      .querySelector(".message-graph")
      .getBoundingClientRect()
    const cards = [
      ...document.querySelectorAll("[data-graph-node] .message-card"),
    ]
    return (
      cards.length > 0 &&
      cards.every((card) => {
        const box = card.getBoundingClientRect()
        return (
          box.left >= bounds.left &&
          box.right <= bounds.right &&
          box.top >= bounds.top &&
          box.bottom <= bounds.bottom
        )
      })
    )
  })
}

async function screenshot(page, label) {
  await page.screenshot({
    path: path.join(tmpdir(), `rostfrei-studio-${label}.png`),
  })
}

async function text(page, selector) {
  return page.$eval(selector, (element) => element.textContent)
}
