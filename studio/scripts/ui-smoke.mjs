import assert from "node:assert/strict"
import { existsSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { spawn } from "node:child_process"

import puppeteer from "puppeteer-core"

const root = fileURLToPath(new URL("..", import.meta.url))
const port = 4176
const url = `http://127.0.0.1:${port}`
const vite = fileURLToPath(
  new URL("../node_modules/vite/bin/vite.js", import.meta.url)
)
const server = spawn(
  process.execPath,
  [vite, "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
  { cwd: root, stdio: "ignore" }
)

let browser
try {
  await waitForServer(url)
  browser = await puppeteer.launch({
    executablePath: chromeExecutable(),
    headless: true,
    args: ["--disable-gpu"],
  })
  await browser
    .defaultBrowserContext()
    .overridePermissions(url, ["clipboard-read", "clipboard-sanitized-write"])
  const page = await browser.newPage()
  await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 1 })

  const pageErrors = []
  page.on("pageerror", (error) => pageErrors.push(error.message))
  await page.goto(url, { waitUntil: "networkidle0" })
  await page.waitForSelector('button[aria-label^="Run "]', { timeout: 10000 })

  await page.evaluate(() => {
    const graph = document.querySelector(".message-graph")
    window.__graphNodeCounts = []
    let previous = -1
    const record = () => {
      const count = document.querySelectorAll("[data-graph-node]").length
      if (count !== previous) {
        window.__graphNodeCounts.push({ count, at: performance.now() })
        previous = count
      }
    }
    record()
    new MutationObserver(record).observe(graph, {
      childList: true,
      subtree: true,
    })
  })

  await page.click('button[aria-label^="Run "]')
  await page.waitForFunction(
    () =>
      [...document.querySelectorAll("[data-slot=badge]")].some((badge) =>
        badge.textContent?.includes("observing")
      ),
    { timeout: 10000 }
  )
  await page.waitForFunction(
    () => {
      const observing = [
        ...document.querySelectorAll("[data-slot=badge]"),
      ].some((badge) => badge.textContent?.includes("observing"))
      return !observing && document.querySelector(".run-row")
    },
    { timeout: 60000 }
  )
  await page.waitForFunction(
    () => {
      const nodeCount = document.querySelectorAll("[data-graph-node]").length
      const contextCount = document.querySelectorAll(
        "[data-graph-node][data-context]"
      ).length
      const edgeCount = document.querySelectorAll("[data-graph-edge]").length
      return nodeCount > 0 && edgeCount === nodeCount - contextCount - 1
    },
    { timeout: 5000 }
  )

  const graph = await page.evaluate(() => {
    const nodes = [...document.querySelectorAll("[data-graph-node]")].map(
      (node) => ({
        id: node.dataset.nodeId,
        parentId: node.dataset.parentId,
        context: Boolean(node.dataset.context),
      })
    )
    const ids = new Set(nodes.map((node) => node.id))
    const edges = [...document.querySelectorAll("[data-graph-edge]")].map(
      (edge) => ({
        sourceId: edge.dataset.sourceId,
        targetId: edge.dataset.targetId,
      })
    )
    return {
      nodes,
      edges,
      unresolved: nodes.filter(
        (node) => node.parentId && !ids.has(node.parentId)
      ),
      counts: window.__graphNodeCounts,
    }
  })

  assert.ok(graph.nodes.length >= 2, "the completed run should render messages")
  assert.equal(graph.unresolved.length, 0, "every causal parent must resolve")
  assert.equal(
    graph.edges.length,
    graph.nodes.filter((node) => !node.context).length - 1,
    "the observed subject message tree should remain connected"
  )
  const progressiveCounts = graph.counts
    .map(({ count }) => count)
    .filter((count, index, counts) => count !== counts[index - 1])
  assert.ok(
    progressiveCounts.includes(2) && progressiveCounts.includes(3),
    `SSE playback was not progressive: ${progressiveCounts.join(", ")}`
  )

  const eventNode = await page.$(
    "[data-graph-node]:not([data-context]) .message-node-domain-event"
  )
  assert.ok(eventNode, "an event node should be available to hover")
  await eventNode.hover()
  await page.waitForSelector(".payload-list", { visible: true })
  const payload = await page.$eval(".payload-list", (element) => ({
    rows: element.querySelectorAll(".payload-row").length,
    text: element.textContent,
    rawJson: Boolean(element.querySelector("pre")),
  }))
  assert.ok(payload.rows > 0, "hover payload should render property rows")
  assert.equal(payload.rawJson, false, "hover payload must not use raw JSON")

  await page.setRequestInterception(true)
  page.on("request", (request) => {
    if (new URL(request.url()).pathname === "/api/tests") {
      void request.respond({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ message: "Use deterministic demo data" }),
      })
      return
    }
    void request.continue()
  })
  await page.reload({ waitUntil: "networkidle0" })
  await page.waitForFunction(() =>
    [...document.querySelectorAll("[data-slot=badge]")].some((badge) =>
      badge.textContent?.includes("demo data")
    )
  )
  await page.waitForFunction(
    () =>
      document.querySelectorAll("[data-graph-node]").length === 7 &&
      document.querySelectorAll("[data-graph-edge]").length === 5
  )
  await page.evaluate(() => {
    window.__layoutSnapshots = []
    let previous = ""
    const record = () => {
      const positions = Object.fromEntries(
        [...document.querySelectorAll("[data-graph-node]")]
          .map((node) => [
            node.dataset.nodeId,
            {
              x: Number(node.dataset.layoutX),
              y: Number(node.dataset.layoutY),
            },
          ])
          .sort(([left], [right]) => left.localeCompare(right))
      )
      const serialized = JSON.stringify(positions)
      if (serialized !== previous) {
        window.__layoutSnapshots.push(positions)
        previous = serialized
      }
    }
    record()
    window.__layoutTimer = window.setInterval(record, 50)
  })

  await page.click('button[aria-label^="Run "]')
  await page.waitForFunction(
    () =>
      [...document.querySelectorAll("[data-slot=badge]")].some((badge) =>
        badge.textContent?.includes("observing")
      ),
    { timeout: 10000 }
  )
  await page.waitForFunction(
    () =>
      ![...document.querySelectorAll("[data-slot=badge]")].some((badge) =>
        badge.textContent?.includes("observing")
      ),
    { timeout: 60000 }
  )
  await page.waitForFunction(
    () =>
      document.querySelectorAll("[data-graph-node]").length === 7 &&
      document.querySelectorAll("[data-graph-edge]").length === 5
  )
  await page.waitForFunction(() => {
    const snapshots = window.__layoutSnapshots
    return (
      snapshots.some((snapshot) => Object.keys(snapshot).length === 2) &&
      Object.keys(snapshots.at(-1) ?? {}).length === 7
    )
  })
  const layoutSnapshots = await page.evaluate(() => {
    window.clearInterval(window.__layoutTimer)
    return window.__layoutSnapshots
  })
  const branchCounts = layoutSnapshots
    .map((snapshot) => Object.keys(snapshot).length)
    .filter((count, index, counts) => count !== counts[index - 1])
  assert.deepEqual(
    branchCounts,
    [7, 2, 3, 4, 5, 6, 7],
    "the branching demo should append one stable node at a time"
  )
  const expectedPositions = {
    "fixture-rent-available-bicycle": { x: 0, y: 0 },
    "command-rent": { x: 280, y: 0 },
    "event-rented": { x: 560, y: 0 },
    "event-audit": { x: 560, y: 140 },
    "integration-started": { x: 840, y: 0 },
    "integration-availability": { x: 840, y: 140 },
    "integration-audit": { x: 840, y: 280 },
  }

  const fixturePresentation = await page.evaluate(() => {
    const fixture = document.querySelector(
      '[data-node-id="fixture-rent-available-bicycle"]'
    )
    const event = document.querySelector('[data-node-id="event-rented"]')
    const eventNodeBounds = event
      .querySelector(".message-node")
      .getBoundingClientRect()
    const eventLabelBounds = event
      .querySelector(".message-node-label")
      .getBoundingClientRect()
    const arrows = [...document.querySelectorAll("[data-graph-edge]")].map(
      (edge) => {
        const line = edge.querySelector(":scope > .graph-edge")
        const marker = edge.querySelector("marker")
        return {
          markerEnd: line?.getAttribute("marker-end"),
          markerId: marker?.id,
          orientation: marker?.getAttribute("orient"),
        }
      }
    )
    return {
      opacity: Number(
        getComputedStyle(fixture.querySelector(".message-node")).opacity
      ),
      fixtureIsDomainEvent: fixture
        .querySelector(".message-node")
        .classList.contains("message-node-domain-event"),
      fixtureLabel: fixture.querySelector(".message-node-label small")
        .textContent,
      eventCenterOffset: Math.abs(
        eventNodeBounds.left +
          eventNodeBounds.width / 2 -
          (eventLabelBounds.left + eventLabelBounds.width / 2)
      ),
      commandIncomingEdges: document.querySelectorAll(
        '[data-graph-edge][data-target-id="command-rent"]'
      ).length,
      leadLines: document.querySelectorAll(".message-node-lead").length,
      arrows,
    }
  })
  assert.ok(
    fixturePresentation.opacity <= 0.4,
    `fixture context should be subdued: ${fixturePresentation.opacity}`
  )
  assert.equal(
    fixturePresentation.fixtureIsDomainEvent,
    true,
    "fixture history should use domain-event message styling"
  )
  assert.equal(
    fixturePresentation.fixtureLabel,
    "domain event series",
    "fixture history should be labelled as a domain-event series"
  )
  assert.ok(
    fixturePresentation.eventCenterOffset <= 1,
    `event labels should be centered under their nodes: ${fixturePresentation.eventCenterOffset}px`
  )
  assert.equal(
    fixturePresentation.commandIncomingEdges,
    0,
    "the subject command should not have an incoming line"
  )
  assert.equal(
    fixturePresentation.leadLines,
    0,
    "the decorative command lead should be removed"
  )
  assert.ok(
    fixturePresentation.arrows.every(
      ({ markerEnd, markerId, orientation }) =>
        markerEnd === `url(#${markerId})` && orientation === "auto"
    ),
    "every edge should use an endpoint-aligned SVG marker"
  )
  for (const [id, expected] of Object.entries(expectedPositions)) {
    const observed = layoutSnapshots.flatMap((snapshot) =>
      snapshot[id] ? [snapshot[id]] : []
    )
    assert.ok(observed.length > 0, `${id} should be rendered`)
    observed.forEach((position) =>
      assert.deepEqual(position, expected, `${id} moved after another append`)
    )
  }

  await new Promise((resolve) => setTimeout(resolve, 700))
  await page.evaluate(() => {
    window.__edgeAnimationStarts = 0
    document.querySelectorAll("[data-graph-edge]").forEach((edge, index) => {
      edge.dataset.stabilityMarker = `edge-${index}`
    })
    document
      .querySelector(".message-graph")
      .addEventListener("animationstart", (event) => {
        if (
          event.target instanceof Element &&
          (event.target.classList.contains("graph-edge") ||
            event.target.classList.contains("graph-edge-arrow"))
        ) {
          window.__edgeAnimationStarts += 1
        }
      })
  })

  await page.click('button[aria-label="Collapse sidebar"]')
  await page.waitForSelector('button[aria-label="Expand sidebar"]')
  await page.click('button[aria-label="Expand sidebar"]')
  await page.waitForSelector('button[aria-label="Collapse sidebar"]')

  const zoomBefore = await page.$eval(".graph-zoom-value", (element) =>
    Number(element.textContent?.replace("%", ""))
  )
  await page.click('button[aria-label="Zoom in"]')
  await page.waitForFunction(
    (previous) =>
      Number(
        document
          .querySelector(".graph-zoom-value")
          ?.textContent?.replace("%", "")
      ) > previous,
    {},
    zoomBefore
  )
  const viewportBeforePan = await page.$eval(
    ".react-flow__viewport",
    (element) => element.style.transform
  )
  const pane = await page.$(".react-flow__pane")
  const paneBounds = await pane?.boundingBox()
  assert.ok(paneBounds, "the React Flow pane should be available")
  const dragStart = { x: paneBounds.x + 70, y: paneBounds.y + 90 }
  await page.mouse.move(dragStart.x, dragStart.y)
  await page.mouse.down()
  await page.mouse.move(dragStart.x + 90, dragStart.y + 45, { steps: 8 })
  await page.mouse.up()
  await page.waitForFunction(
    (previous) =>
      document.querySelector(".react-flow__viewport")?.style.transform !==
      previous,
    {},
    viewportBeforePan
  )
  const viewportAfterPan = await page.$eval(
    ".react-flow__viewport",
    (element) => element.style.transform
  )
  await page.click('button[aria-label="Fit graph to view"]')
  await page.waitForFunction(
    (previous) =>
      document.querySelector(".react-flow__viewport")?.style.transform !==
      previous,
    {},
    viewportAfterPan
  )

  const branchEvent = await page.$(
    '[data-node-id="event-rented"] .message-node'
  )
  assert.ok(branchEvent, "a branching event should be available to hover")
  await branchEvent.hover()
  await page.waitForSelector("[data-node-popup]", { visible: true })
  await page.click('[data-node-id="event-rented"] .message-node')
  await new Promise((resolve) => setTimeout(resolve, 300))
  const pinnedState = await page.evaluate(() => ({
    popup: document
      .querySelector("[data-node-popup]")
      ?.getAttribute("data-popup-pinned"),
    expanded: document
      .querySelector('[data-node-id="event-rented"] .message-node')
      ?.getAttribute("aria-expanded"),
  }))
  assert.deepEqual(
    pinnedState,
    { popup: "true", expanded: "true" },
    "clicking an open hover preview should pin it"
  )
  await page.mouse.move(8, 8)
  await new Promise((resolve) => setTimeout(resolve, 250))
  assert.ok(
    await page.$("[data-node-popup]"),
    "a clicked message popup should stay open after the pointer leaves"
  )
  const eventIdentityControls = await page.$$eval(
    "[data-node-popup] [data-copy-identity]",
    (buttons) =>
      buttons.map((button) => ({
        label: button.getAttribute("data-copy-identity"),
        top: button.getBoundingClientRect().top,
      }))
  )
  assert.deepEqual(
    eventIdentityControls.map(({ label }) => label),
    ["message ID", "cause ID"],
    "event popups should expose every causal identity as a copy control"
  )
  assert.equal(
    new Set(eventIdentityControls.map(({ top }) => Math.round(top))).size,
    1,
    "event identity controls should stay on one row"
  )
  const copyEventMessageId = await page.$('[data-copy-identity="message ID"]')
  await copyEventMessageId.click()
  await page.waitForFunction(
    () =>
      document
        .querySelector('[data-copy-identity="message ID"]')
        ?.textContent?.trim() === "copied"
  )
  assert.equal(
    await page.evaluate(() => navigator.clipboard.readText()),
    "evt_01HZX8B8A2",
    "the event message ID control should copy the hidden identity"
  )
  await page.mouse.click(paneBounds.x + 24, paneBounds.y + 24)
  await page.waitForSelector("[data-node-popup]", { hidden: true })

  const commandNode = await page.$(
    '[data-node-id="command-rent"] .message-node'
  )
  assert.ok(commandNode, "the command node should be available to click")
  await commandNode.click()
  await page.waitForSelector("[data-command-response]", { visible: true })
  const commandResponse = await page.$eval(
    "[data-command-response]",
    (element) => ({
      rows: element.querySelectorAll(".payload-row").length,
      text: element.textContent,
      rawJson: Boolean(element.querySelector("pre")),
    })
  )
  assert.ok(commandResponse.rows > 0, "the command response should render rows")
  assert.ok(
    commandResponse.text?.includes("accepted"),
    "the command response should include its decision"
  )
  assert.equal(
    commandResponse.rawJson,
    false,
    "the command response must not use raw JSON"
  )
  assert.equal(
    commandResponse.text?.includes("cmd_01HZX8B7T7"),
    false,
    "technical IDs should not be printed in the popup"
  )
  const commandPopup = await page.$eval("[data-node-popup]", (element) => ({
    copyControls: element.querySelectorAll("[data-copy-identity]").length,
    text: element.textContent,
    width: element.getBoundingClientRect().width,
  }))
  assert.equal(
    commandPopup.copyControls,
    0,
    "command popups should omit technical identity controls"
  )
  assert.equal(
    commandPopup.text?.includes("commandMessageId") ||
      commandPopup.text?.includes("responseMessageId"),
    false,
    "command response identity fields should be omitted"
  )
  assert.equal(
    /\bv1\b/.test(commandPopup.text ?? ""),
    false,
    "message popups should not show a schema-version badge"
  )
  assert.ok(
    commandPopup.width >= 340,
    `the command popup should leave room for response fields: ${commandPopup.width}`
  )
  const eventColors = await page.evaluate(() => ({
    domain: getComputedStyle(
      document.querySelector(".message-node-domain-event")
    ).backgroundImage,
    integration: getComputedStyle(
      document.querySelector(".message-node-integration-event")
    ).backgroundImage,
  }))
  assert.ok(
    eventColors.domain.includes("233, 188, 105"),
    `domain events should use the yellow palette: ${eventColors.domain}`
  )
  assert.ok(
    eventColors.integration.includes("114, 202, 221"),
    `integration events should retain a distinct cyan palette: ${eventColors.integration}`
  )
  await page.mouse.click(paneBounds.x + 24, paneBounds.y + 24)
  await page.waitForSelector("[data-node-popup]", { hidden: true })
  const edgeStability = await page.evaluate(() => ({
    animationStarts: window.__edgeAnimationStarts,
    markers: [...document.querySelectorAll("[data-graph-edge]")].map(
      (edge) => edge.dataset.stabilityMarker
    ),
  }))
  assert.equal(
    edgeStability.animationStarts,
    0,
    "graph interactions should not restart edge animations"
  )
  assert.deepEqual(
    edgeStability.markers,
    ["edge-0", "edge-1", "edge-2", "edge-3", "edge-4"],
    "graph interactions should preserve the existing edge elements"
  )

  await page.evaluate(() => {
    const key = "rostfrei-tracer-studio-runs-v1"
    const [template] = JSON.parse(localStorage.getItem(key) ?? "[]")
    if (!template)
      throw new Error("a stored run is required for scroll testing")
    const runs = Array.from({ length: 16 }, (_, index) => ({
      ...template,
      runId: `scroll-test-${index}`,
      createdAt: new Date(Date.now() - index * 60_000).toISOString(),
    }))
    localStorage.setItem(key, JSON.stringify(runs))
  })
  await page.reload({ waitUntil: "networkidle0" })
  await page.waitForFunction(
    () => document.querySelectorAll(".run-row").length === 16
  )
  const runScroll = await page.$eval(".past-runs-scroll", (element) => {
    const style = getComputedStyle(element)
    const sidebarStyle = getComputedStyle(
      document.querySelector(".studio-sidebar")
    )
    const trackStyle = getComputedStyle(element, "::-webkit-scrollbar-track")
    return {
      overflowY: style.overflowY,
      scrollable: element.scrollHeight > element.clientHeight,
      scrollbarColor: style.scrollbarColor,
      trackBackground: trackStyle.backgroundColor,
      sidebarOverflowY: sidebarStyle.overflowY,
    }
  })
  assert.equal(runScroll.overflowY, "auto", "Past Runs should own scrolling")
  assert.equal(runScroll.scrollable, true, "many Past Runs should overflow")
  assert.notEqual(
    runScroll.scrollbarColor,
    "auto",
    "the Past Runs scrollbar should be themed"
  )
  assert.equal(
    runScroll.trackBackground,
    "rgba(0, 0, 0, 0)",
    "the Past Runs scrollbar track should be transparent"
  )
  assert.equal(
    runScroll.sidebarOverflowY,
    "hidden",
    "the whole sidebar should not scroll"
  )

  await page.click('[data-node-id="command-rent"] .message-node')
  await page.waitForSelector("[data-command-response]", { visible: true })
  const screenshot = path.join(tmpdir(), "rostfrei-tracer-studio-smoke.png")
  await page.screenshot({ path: screenshot })

  await page.evaluate(() => {
    const button = [...document.querySelectorAll(".test-row button")].find(
      (candidate) => candidate.textContent?.includes("Return a rented bicycle")
    )
    button.click()
  })
  await page.waitForFunction(
    () =>
      document.querySelectorAll("[data-graph-node][data-context]").length === 2
  )
  const setupContext = await page.evaluate(() => ({
    ids: [...document.querySelectorAll("[data-graph-node][data-context]")].map(
      (node) => node.dataset.nodeId
    ),
    subjectIncomingEdges: document.querySelectorAll(
      '[data-graph-edge][data-target-id="preview-command-return-rented-bicycle"]'
    ).length,
    mutedEdges: document.querySelectorAll(".graph-edge-context").length,
  }))
  assert.deepEqual(
    setupContext.ids,
    ["fixture-return-rented-bicycle", "setup-command-return-rented-bicycle-0"],
    "declared fixture setup should appear as subdued context"
  )
  assert.equal(
    setupContext.subjectIncomingEdges,
    0,
    "fixture setup should not draw an incoming edge into the subject command"
  )
  assert.equal(
    setupContext.mutedEdges,
    1,
    "fixture setup edges should use the muted context style"
  )

  await page.setViewport({ width: 390, height: 844, deviceScaleFactor: 1 })
  await page.reload({ waitUntil: "networkidle0" })
  await page.waitForSelector("[data-graph-node]")
  const offscreenNodes = await page.$$eval(
    "[data-graph-node]",
    (nodes) =>
      nodes.filter((node) => {
        const bounds = node.getBoundingClientRect()
        return (
          bounds.x < 0 ||
          bounds.right > innerWidth ||
          bounds.y < 0 ||
          bounds.bottom > innerHeight
        )
      }).length
  )
  assert.equal(
    offscreenNodes,
    0,
    "message dots should remain inside the mobile viewport"
  )
  assert.deepEqual(pageErrors, [], `browser errors: ${pageErrors.join("; ")}`)

  console.log(
    JSON.stringify({
      nodes: graph.nodes.length,
      edges: graph.edges.length,
      progressiveCounts,
      branchCounts,
      payloadRows: payload.rows,
      commandResponseRows: commandResponse.rows,
      edgeAnimationRestarts: edgeStability.animationStarts,
      pastRuns: 16,
      screenshot,
    })
  )
} finally {
  await browser?.close()
  server.kill("SIGTERM")
}

async function waitForServer(serverUrl) {
  const deadline = Date.now() + 10000
  while (Date.now() < deadline) {
    try {
      const response = await fetch(serverUrl)
      if (response.ok) return
    } catch {
      // Vite is still starting.
    }
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  throw new Error(`Vite did not start at ${serverUrl}`)
}

function chromeExecutable() {
  const candidates = [
    process.env.CHROME_BIN,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
  ].filter(Boolean)
  const executable = candidates.find((candidate) => existsSync(candidate))
  if (!executable) throw new Error("Set CHROME_BIN to a Chrome executable")
  return executable
}
