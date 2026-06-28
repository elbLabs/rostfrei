import assert from "node:assert/strict"
import { existsSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { spawn } from "node:child_process"

import puppeteer from "puppeteer-core"

const root = fileURLToPath(new URL("..", import.meta.url))
const url = "http://127.0.0.1:4176"
const vite = fileURLToPath(
  new URL("../node_modules/vite/bin/vite.js", import.meta.url)
)
const server = spawn(
  process.execPath,
  [vite, "--host", "127.0.0.1", "--port", "4176", "--strictPort"],
  { cwd: root, stdio: "ignore" }
)

// All Tracer requests are intercepted before navigation. This test never resets
// a real scenario or publishes a real command, even if a Tracer is running.
let apiMode = "demo"
let sample
let failExpectations = false
let disconnectRun = false
const publications = []
let browser
let page

try {
  await waitForServer()
  browser = await puppeteer.launch({
    executablePath: chromeExecutable(),
    headless: true,
    args: process.env.CHROME_NO_SANDBOX === "1" ? ["--no-sandbox"] : [],
  })
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
  await ready(page)
  assert.equal(
    await page.$eval('[data-layout="canvas"]', (button) =>
      button.getAttribute("aria-pressed")
    ),
    "true"
  )
  await page.keyboard.press("2")
  await page.waitForSelector('[data-layout="workbench"][aria-pressed="true"]')
  for (const tag of ["input", "textarea", "div"]) {
    await page.evaluate((tag) => {
      const editor = document.createElement(tag)
      editor.id = "shortcut-editor"
      if (tag === "div") editor.contentEditable = "true"
      document.body.append(editor)
      editor.focus()
    }, tag)
    await page.keyboard.press("1")
    assert.equal(
      await page.$eval('[data-layout="workbench"]', (button) =>
        button.getAttribute("aria-pressed")
      ),
      "true",
      `typing in ${tag} must not switch layouts`
    )
    await page.evaluate(() =>
      document.getElementById("shortcut-editor").remove()
    )
  }

  assert.match(await text(page, ".connection-status"), /Demo data/)
  assert.match(
    await text(page, ".execution-result"),
    /Ready to run.*expected messages/s
  )
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    4
  )
  assert.match(await text(page, "[data-command-response]"), /Expected outcome/)
  assert.doesNotMatch(
    await text(page, ".message-inspector"),
    /Response\s+status\s+accepted/
  )
  await assertFitted(page)
  await screenshot(page, "expected")

  sample = await page.evaluate(async () => {
    const { SAMPLE_TESTS, SAMPLE_DEFINITIONS, SAMPLE_FIXTURE } =
      await import("/src/lib/sample-data.ts")
    return {
      tests: SAMPLE_TESTS,
      definitions: SAMPLE_DEFINITIONS,
      fixture: SAMPLE_FIXTURE,
    }
  })

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
  await page.click(".run-button")
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
    7
  )
  assert.equal(
    await page.$$eval("[data-graph-edge]", (edges) => edges.length),
    6
  )
  assert.match(
    await text(page, ".execution-result"),
    /Command accepted as expected/
  )
  assert.match(await text(page, ".run-button"), /Run again/)
  await assertFitted(page)
  await screenshot(page, "observed")

  // The experimental layout must preserve the current run and make it easy
  // to return to the established workbench, including message selection.
  const runBeforeLayoutSwitch = await text(page, ".execution-result strong")
  const workbenchArea = await page.$eval(
    ".message-graph",
    (element) => element.clientWidth * element.clientHeight
  )
  await page.keyboard.press("1")
  await page.waitForSelector(".message-inspector", { hidden: true })
  await assertFitted(page)
  const canvasArea = await page.$eval(
    ".message-graph",
    (element) => element.clientWidth * element.clientHeight
  )
  assert.ok(
    canvasArea > workbenchArea * 1.5,
    "Canvas should give substantially more room to the flow"
  )
  assert.equal(
    await text(page, ".execution-result strong"),
    runBeforeLayoutSwitch
  )
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    7
  )
  await screenshot(page, "canvas")
  await page.click(".scenario-toggle")
  await page.waitForSelector(".scenario-summary", { visible: true })
  assert.match(await text(page, ".scenario-summary"), /bicycle_id: bike-42/)
  await page.click(".scenario-toggle")
  await page.waitForSelector(".scenario-summary", { hidden: true })
  await page.click('[data-message-id="event-rented"]')
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
      document.activeElement?.getAttribute("data-message-id") === "event-rented"
  )
  await page.keyboard.press("Enter")
  await page.waitForSelector(".message-inspector", { visible: true })
  await page.keyboard.press("2")
  await page.waitForSelector(".scenario-summary", { visible: true })
  await page.waitForSelector(".studio-sidebar", { visible: true })
  assert.equal(
    await text(page, ".inspector-message-heading h2"),
    "bicycle-rented"
  )
  assert.equal(
    await text(page, ".execution-result strong"),
    runBeforeLayoutSwitch
  )
  await assertFitted(page)

  await page.click('[data-message-id="event-rented"]')
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
    "evt_01HZX8B8A2"
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
  await page.click('button[aria-label="Fit graph to view"]')
  await assertFitted(page)
  assert.equal(
    await text(page, ".inspector-message-heading h2"),
    "bicycle-rented",
    "viewport controls preserve message selection"
  )
  await page.focus('[data-message-id="fixture-event-0-demo-fleet-imported"]')
  await page.keyboard.press("Enter")
  await page.waitForFunction(
    () =>
      document.querySelector(".inspector-message-heading h2").textContent ===
      "rental-fleet-imported"
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
  await page.click(".run-button")
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
  await page.click(".run-button")
  await page.waitForFunction(
    () => document.querySelectorAll(".run-row").length === 3
  )
  await passed(page)
  assert.match(await text(page, ".run-button"), /Run again/)

  // Reloaded history retains its definition, outcome, and simulated provenance.
  await page.reload({ waitUntil: "networkidle0" })
  await ready(page)
  await selectRun(page, "Reject a maintenance-required bicycle")
  assert.match(
    await text(page, ".execution-result"),
    /Command rejected as expected/
  )
  assert.match(await text(page, ".connection-status"), /Demo data/)

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

  // Exercise the connected UI against deterministic HTTP fixtures.
  apiMode = "live"
  await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 1 })
  await page.reload({ waitUntil: "networkidle0" })
  await ready(page)
  assert.match(await text(page, ".connection-status"), /Isolated Test/)
  await selectRun(page, "Rent an available bicycle")
  assert.match(await text(page, ".connection-status"), /Demo data/)
  assert.match(
    await text(page, ".run-button"),
    /Run in Test/,
    "a demo history entry must not conceal the next run's live Test target"
  )
  await page.click(".run-button")
  await passed(page)
  assert.match(await text(page, ".connection-status"), /Isolated Test/)
  assert.equal(publications.at(-1), "rent-available-bicycle")

  await selectTest(page, "Reject a maintenance-required bicycle")
  await page.click(".run-button")
  await passed(page)
  await selectTest(page, "Return a rented bicycle")
  await selectRun(page, "Reject a maintenance-required bicycle")
  await page.click(".run-button")
  await passed(page)
  assert.equal(
    publications.at(-1),
    "reject-unavailable-bicycle",
    "rerunning history must execute its own test, not the previously viewed definition"
  )
  assert.match(await text(page, ".message-inspector"), /bike-99/)

  failExpectations = true
  await selectTest(page, "Rent an available bicycle")
  await page.click(".run-button")
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
  await page.click(".run-button")
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
  await page.click(".run-button")
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
    ["demo-fleet-imported", "fixture-second"],
  ])
  assert.equal(semantics.contextEdges, 1)
  assert.equal(
    semantics.unresolvedEdges,
    0,
    "observation order must not invent a causal arrow"
  )
  assert.equal(
    semantics.missingOutcome,
    null,
    "a completed operation alone does not prove command acceptance"
  )
  assert.deepEqual(semantics.scalarFacts, [
    [["Value", "null"]],
    [["Value", "false"]],
    [["Value", "0"]],
    [["Value", '\"\"']],
  ])

  await page.click('[data-layout="canvas"]')
  for (const width of [1280, 390]) {
    await page.setViewport({ width, height: 844, deviceScaleFactor: 1 })
    await page.reload({ waitUntil: "networkidle0" })
    await ready(page)
    assert.equal(
      await page.$eval('[data-layout="canvas"]', (button) =>
        button.getAttribute("aria-pressed")
      ),
      "true",
      "the chosen layout should persist across reloads"
    )
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

  apiMode = "empty"
  await page.reload({ waitUntil: "networkidle0" })
  await page.waitForSelector(".graph-empty")
  assert.match(
    await text(page, ".execution-header"),
    /No behavioral tests are registered/
  )
  assert.equal(
    await page.$eval(".run-button", (button) => button.disabled),
    true
  )
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    0
  )
  assert.deepEqual(errors, [], "no browser runtime errors")
  console.log(
    `PASS: expected/observed flows; accepted/rejected outcomes; reruns and history; persistent inspector and clipboard; causal edges; 6 responsive widths; mocked connected runs, failed expectations, ambiguous response and storage failure; empty catalog. Screenshots: ${path.join(tmpdir(), "rostfrei-studio-*.png")}`
  )
} catch (error) {
  if (page) {
    await screenshot(page, "failure")
    console.error(await text(page, ".execution-header"))
  }
  throw error
} finally {
  await browser?.close()
  server.kill("SIGTERM")
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
  if (apiMode === "demo")
    return respond({ message: "Deterministic demo data" }, 503)
  if (pathname === "/api/tests")
    return respond({ items: apiMode === "empty" ? [] : sample.tests })
  if (pathname === "/api/test-scenario/fixtures/demo-fleet")
    return respond(sample.fixture)
  const match = /^\/api\/tests\/([^/]+)(\/runs)?$/.exec(pathname)
  if (match && sample.definitions[match[1]]) {
    if (!match[2]) return respond(sample.definitions[match[1]])
    assert.equal(request.method(), "POST")
    publications.push(match[1])
    if (disconnectRun) return request.abort("failed")
    return respond(reportFor(match[1]))
  }
  return respond({ message: "Unexpected test endpoint" }, 404)
}

function reportFor(testId) {
  const definition = sample.definitions[testId].definition
  const expectedNodes = definition.expected.graphs[0].nodes
  const root = expectedNodes.find(
    (node) => node.kind === "command" && !node.parentKey
  )
  const accepted = root.outcome === "accepted"
  const identity = `mock-${publications.length}`
  const commandOutcome = {
    responseMessageId: `${identity}-response`,
    commandMessageId: `${identity}-${root.key}`,
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
  const messages = (failExpectations ? [root] : expectedNodes).map(
    (node, index) => ({
      kind: node.kind,
      name: node.name,
      schemaVersion: node.schemaVersion,
      payload: node.payload,
      context: node.context,
      messageId: `${identity}-${node.key}`,
      causationId: node.parentKey ? `${identity}-${node.parentKey}` : undefined,
      correlationId: identity,
      observationOrder: index + 1,
    })
  )
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
      matches: messages.map((message) => ({
        expectedKey: message.messageId.slice(identity.length + 1),
        observedMessageId: message.messageId,
      })),
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
}

async function selectRun(page, name) {
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
  await ready(page)
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

async function waitForServer() {
  const deadline = Date.now() + 20000
  while (Date.now() < deadline) {
    try {
      if ((await fetch(url)).ok) return
    } catch {
      /* Vite is starting. */
    }
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  throw new Error(`Vite did not start at ${url}`)
}

function chromeExecutable() {
  const executable = [
    process.env.CHROME_BIN,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
  ]
    .filter(Boolean)
    .find((candidate) => existsSync(candidate))
  if (!executable) throw new Error("Set CHROME_BIN to a Chrome executable")
  return executable
}
