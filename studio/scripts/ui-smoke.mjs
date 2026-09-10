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
  const forceDemo = (request) => {
    if (new URL(request.url()).pathname === "/api/tests") {
      void request.respond({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ message: "Use deterministic demo data" }),
      })
      return
    }
    void request.continue()
  }
  await page.setRequestInterception(true)
  page.on("request", forceDemo)
  await page.goto(url, { waitUntil: "networkidle0" })
  await page.waitForSelector('button[aria-label^="Run "]', { timeout: 10000 })
  await new Promise((resolve) => setTimeout(resolve, 700))

  await page.evaluate(() => {
    const graph = document.querySelector(".message-graph")
    window.__graphNodeCounts = []
    window.__arrivalTimeline = []
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
    graph.addEventListener("animationstart", (event) => {
      if (!(event.target instanceof Element)) return
      if (event.animationName === "node-arrive") {
        window.__arrivalTimeline.push({
          kind: "node",
          id: event.target.dataset.nodeId,
          at: performance.now(),
        })
      } else if (event.animationName === "edge-reveal") {
        window.__arrivalTimeline.push({
          kind: "edge",
          id: event.target.closest("[data-graph-edge]")?.dataset.targetId,
          at: performance.now(),
        })
      }
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
      return nodeCount > 0 && contextCount > 0 && edgeCount === nodeCount - 1
    },
    { timeout: 5000 }
  )
  await new Promise((resolve) => setTimeout(resolve, 700))

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
      arrivals: window.__arrivalTimeline,
    }
  })

  assert.ok(graph.nodes.length >= 2, "the completed run should render messages")
  assert.equal(graph.unresolved.length, 0, "every causal parent must resolve")
  assert.equal(
    graph.edges.length,
    graph.nodes.length - 1,
    "canonical fixture context and observed messages should remain connected"
  )
  assert.deepEqual(
    graph.arrivals,
    [],
    "nodes and lines should paint atomically without replaying arrival animations"
  )
  const progressiveCounts = graph.counts
    .map(({ count }) => count)
    .filter((count, index, counts) => count !== counts[index - 1])
  assert.ok(
    progressiveCounts.includes(3) && progressiveCounts.includes(4),
    `SSE playback was not progressive: ${progressiveCounts.join(", ")}`
  )

  const eventNode = await page.$(
    "[data-graph-node]:not([data-context]) .message-node-domain-event"
  )
  assert.ok(eventNode, "an event node should be available to inspect")
  await eventNode.hover()
  await new Promise((resolve) => setTimeout(resolve, 250))
  assert.equal(
    await page.$("[data-node-popup]"),
    null,
    "hovering a message should not repeatedly create and remove its details"
  )
  await eventNode.click()
  await page.waitForSelector(".payload-list", { visible: true })
  await page.mouse.move(8, 8)
  await new Promise((resolve) => setTimeout(resolve, 250))
  assert.ok(
    await page.$("[data-node-popup]"),
    "clicked message details should stay open after the pointer leaves"
  )
  const payload = await page.$eval(".payload-list", (element) => ({
    rows: element.querySelectorAll(".payload-row").length,
    text: element.textContent,
    rawJson: Boolean(element.querySelector("pre")),
  }))
  assert.ok(payload.rows > 0, "hover payload should render property rows")
  assert.equal(payload.rawJson, false, "hover payload must not use raw JSON")

  page.off("request", forceDemo)
  const previewRequests = []
  const testRequests = []
  const testSeriesRequests = []
  const ambiguousTestRequests = []
  page.on("request", (request) => {
    const pathname = new URL(request.url()).pathname
    if (pathname === "/api/tests") {
      void request.respond({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ message: "Use deterministic demo data" }),
      })
      return
    }
    if (pathname === "/api/catalog") {
      void request.respond({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          catalogVersion: 1,
          contexts: [
            {
              id: "bike-rental",
              label: "Bike Rental",
              aggregates: [
                {
                  id: "rental-fleet",
                  label: "Rental fleet",
                  aggregateType: "bike-rental/rental-fleet",
                  testInstancesHref:
                    "/contexts/bike-rental/aggregates/rental-fleet/instances",
                },
              ],
              commands: [
                {
                  id: "rent-bicycle",
                  label: "Rent bicycle",
                  versions: [
                    {
                      schemaVersion: 2,
                      contentType: "application/json",
                      fields: [
                        { name: "fleet_id", value: { kind: "opaque" } },
                        {
                          name: "bicycle_id",
                          value: { kind: "opaque" },
                        },
                        {
                          name: "request_id",
                          value: { kind: "opaque" },
                        },
                        {
                          name: "attempt",
                          value: { kind: "scalar", scalar: "u64" },
                        },
                      ],
                      payloadTemplate: {
                        fleet_id: null,
                        bicycle_id: null,
                        request_id: null,
                        attempt: 0,
                      },
                      testInputsHrefTemplate:
                        "/contexts/bike-rental/commands/rent-bicycle/schemas/2/inputs",
                      simulateHrefTemplate:
                        "/contexts/bike-rental/commands/rent-bicycle/simulate",
                      testHrefTemplate:
                        "/contexts/bike-rental/commands/rent-bicycle/test",
                    },
                  ],
                },
              ],
            },
          ],
        }),
      })
      return
    }
    if (
      pathname === "/api/contexts/bike-rental/commands/rent-bicycle/test" &&
      request.method() === "POST"
    ) {
      testRequests.push({
        body: request.postData(),
        idempotencyKey: request.headers()["idempotency-key"],
        authorization: request.headers().authorization,
      })
      if (testRequests.length > 1) {
        void request.respond({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({ message: "Upstream response unavailable" }),
        })
        return
      }
      void request.respond({
        status: 202,
        contentType: "application/json",
        headers: { location: "/operation-results/confirmed-test" },
        body: JSON.stringify(operationSnapshot("queued", "test")),
      })
      return
    }
    if (pathname === "/api/ambiguous/city-fleet/test") {
      ambiguousTestRequests.push({
        body: request.postData(),
        idempotencyKey: request.headers()["idempotency-key"],
      })
      void request.respond({
        status: 202,
        contentType: "application/json",
        body: JSON.stringify(operationSnapshot("queued", "test")),
      })
      return
    }
    if (
      pathname === "/api/contexts/bike-rental/aggregates/rental-fleet/instances"
    ) {
      void request.respond({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          items: [{ aggregateId: "city-fleet", streamVersion: 1 }],
        }),
      })
      return
    }
    if (
      pathname ===
      "/api/contexts/bike-rental/commands/rent-bicycle/schemas/2/inputs"
    ) {
      void request.respond({
        status: 200,
        contentType: "application/json",
        body: `{"fields":[{"name":"fleet_id","label":"Fleet","options":[{"value":"city-fleet","label":"city-fleet"}]},{"name":"bicycle_id","label":"Bicycle","options":[{"value":"bike-42","label":"bike-42","description":"Available and serviceable"}]},{"name":"attempt","label":"Attempt","options":[{"value":9007199254740993,"label":"9007199254740993"}]}]}`,
      })
      return
    }
    if (
      pathname === "/api/contexts/bike-rental/commands/rent-bicycle/simulate" &&
      request.method() === "POST"
    ) {
      previewRequests.push({
        body: request.postData(),
        idempotencyKey: request.headers()["idempotency-key"],
        authorization: request.headers().authorization,
      })
      void request.respond({
        status: 202,
        contentType: "application/json",
        headers: { location: "/operations/studio-preview" },
        body: JSON.stringify(operationSnapshot("queued")),
      })
      return
    }
    if (pathname === "/api/operations/studio-preview") {
      void request.respond({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(operationSnapshot("completed")),
      })
      return
    }
    if (pathname === "/api/operations/studio-preview/message-series") {
      void request.respond({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          operationId: "studio-preview",
          correlationId: "studio-correlation",
          mode: "simulate",
          messageSeries: {
            messages: [
              {
                kind: "command",
                messageId: "studio-command",
                correlationId: "studio-correlation",
                observationOrder: 0,
                name: "rent-bicycle",
                schemaVersion: 2,
                context: "bike-rental",
                payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
              },
              {
                kind: "domain-event",
                messageId: "studio-event",
                correlationId: "studio-correlation",
                causationId: "studio-command",
                observationOrder: 1,
                name: "bicycle-rented",
                schemaVersion: 1,
                aggregate: {
                  type: "bike-rental/rental-fleet",
                  id: "city-fleet",
                },
                payload: { bicycle_id: "bike-42" },
              },
              {
                kind: "command",
                messageId: "studio-downstream-command",
                correlationId: "studio-correlation",
                causationId: "studio-event",
                observationOrder: 2,
                name: "record-rental-audit",
                schemaVersion: 2,
                aggregate: {
                  type: "bike-rental/rental-audit",
                  id: "city-fleet",
                },
                payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
              },
              {
                kind: "integration-event",
                messageId: "studio-unlinked-event",
                correlationId: "studio-correlation",
                observationOrder: 3,
                name: "rental-observation-incomplete",
                schemaVersion: 1,
                payload: { bicycle_id: "bike-42" },
              },
            ],
            commandOutcomes: [
              {
                responseMessageId: "studio-response",
                commandMessageId: "studio-command",
                correlationId: "studio-correlation",
                observationOrder: 4,
                outcome: { status: "accepted", value: null },
              },
            ],
          },
          capture: {
            settled: true,
            settledFor: "500ms",
            fidelity: "grouped",
            note: "Preview messages use synthetic identities and are grouped with the operation; only explicit causation links are causal",
          },
        }),
      })
      return
    }
    if (pathname === "/api/operation-results/confirmed-test") {
      void request.respond({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ...operationSnapshot("completed", "test"),
          messageSeriesHref: "/captures/confirmed-test-series",
        }),
      })
      return
    }
    if (pathname === "/api/captures/confirmed-test-series") {
      testSeriesRequests.push(new URL(request.url()).search)
      void request.respond({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(testMessageSeries()),
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
      document.querySelectorAll("[data-graph-node]").length === 5 &&
      document.querySelectorAll("[data-graph-edge]").length === 4
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
      document.querySelectorAll("[data-graph-node]").length === 5 &&
      document.querySelectorAll("[data-graph-edge]").length === 4
  )
  await page.waitForFunction(() => {
    const snapshots = window.__layoutSnapshots
    return (
      snapshots.some((snapshot) => Object.keys(snapshot).length === 3) &&
      Object.keys(snapshots.at(-1) ?? {}).length === 5
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
    [5, 3, 4, 5],
    "the canonical demo should append one stable node at a time"
  )
  const expectedPositions = {
    "fixture-event-0-demo-bike-42-added": { x: 0, y: 0 },
    "fixture-event-0-demo-bike-99-added": { x: 280, y: 0 },
    "command-rent": { x: 560, y: 0 },
    "event-rented": { x: 840, y: 0 },
    "integration-started": { x: 1120, y: 0 },
  }

  const fixturePresentation = await page.evaluate(() => {
    const fixture = document.querySelector(
      '[data-node-id="fixture-event-0-demo-bike-99-added"]'
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
          relationship: edge.dataset.edgeRelationship,
        }
      }
    )
    return {
      opacity: Number(
        getComputedStyle(fixture.querySelector(".message-node")).opacity
      ),
      filter: getComputedStyle(fixture.querySelector(".message-node")).filter,
      fixtureIsDomainEvent: fixture
        .querySelector(".message-node")
        .classList.contains("message-node-domain-event"),
      fixtureLabel: fixture.querySelector(".message-node-label small")
        .textContent,
      fixtureName: fixture.querySelector(".message-node-label span")
        .textContent,
      fixtureStreamVersion: fixture.dataset.streamVersion,
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
      edgeAlignmentError: (() => {
        const endpoint = (sourceId, targetId, end) => {
          const path = document.querySelector(
            `[data-source-id="${sourceId}"][data-target-id="${targetId}"] .graph-edge`
          )
          const matrix = path?.getScreenCTM()
          if (!path || !matrix) return null
          const point = path.getPointAtLength(end ? path.getTotalLength() : 0)
          return point.matrixTransform(matrix)
        }
        const fixtureBounds = fixture
          .querySelector(".message-node")
          .getBoundingClientRect()
        const command = document
          .querySelector('[data-node-id="command-rent"] .message-node')
          .getBoundingClientRect()
        const contextStart = endpoint(
          "fixture-event-0-demo-bike-99-added",
          "command-rent",
          false
        )
        const contextEnd = endpoint(
          "fixture-event-0-demo-bike-99-added",
          "command-rent",
          true
        )
        if (!contextStart || !contextEnd) return null
        return Math.max(
          Math.abs(contextStart.x - fixtureBounds.right),
          Math.abs(
            contextStart.y - (fixtureBounds.top + fixtureBounds.height / 2)
          ),
          Math.abs(contextEnd.x - command.left),
          Math.abs(contextEnd.y - (command.top + command.height / 2))
        )
      })(),
    }
  })
  assert.equal(
    fixturePresentation.opacity,
    1,
    "fixture nodes should be opaque so edges cannot show through them"
  )
  assert.notEqual(
    fixturePresentation.filter,
    "none",
    "fixture nodes should remain visually subdued without transparency"
  )
  assert.ok(
    fixturePresentation.edgeAlignmentError !== null &&
      fixturePresentation.edgeAlignmentError <= 1,
    `fixture edges should meet the visible node boundaries: ${fixturePresentation.edgeAlignmentError}`
  )
  assert.equal(
    fixturePresentation.fixtureIsDomainEvent,
    true,
    "fixture messages should use domain-event message styling"
  )
  assert.equal(
    fixturePresentation.fixtureLabel,
    "fixture domain event",
    "fixture context should identify each canonical domain event"
  )
  assert.equal(
    fixturePresentation.fixtureName,
    "bicycle-added",
    "fixture context should use the canonical fixture domain event"
  )
  assert.equal(
    fixturePresentation.fixtureStreamVersion,
    "2",
    "fixture context should retain the recorded stream version"
  )
  assert.ok(
    fixturePresentation.eventCenterOffset <= 1,
    `event labels should be centered under their nodes: ${fixturePresentation.eventCenterOffset}px`
  )
  const fixtureTopology = await page.evaluate(async () => {
    const { expectedGraph, layoutMessageGraph, reportGraph } =
      await import("/src/lib/graph.ts")
    const definition = {
      schemaVersion: 2,
      id: "fixture-topology",
      name: "Fixture topology",
      setup: { fixture: "fixture-history" },
      expected: {
        within: "1s",
        settleFor: "100ms",
        graphs: [
          {
            nodes: [
              {
                kind: "command",
                key: "subject",
                name: "subject-command",
                schemaVersion: 2,
                context: "context",
                payload: {},
                outcome: "accepted",
              },
            ],
          },
        ],
      },
    }
    const fixture = {
      schemaVersion: 1,
      id: "fixture-history",
      revision: "1",
      messages: [
        {
          kind: "domain-event",
          messageId: "fixture-one",
          correlationId: "fixture:fixture-history:1",
          name: "fixture-started",
          schemaVersion: 1,
          aggregate: { type: "context/aggregate", id: "aggregate-1" },
          streamVersion: 1,
          payload: {},
        },
        {
          kind: "domain-event",
          messageId: "fixture-two",
          correlationId: "fixture:fixture-history:1",
          name: "fixture-continued",
          schemaVersion: 1,
          aggregate: { type: "context/aggregate", id: "aggregate-1" },
          streamVersion: 2,
          payload: {},
        },
        {
          kind: "domain-event",
          messageId: "other-fixture-one",
          correlationId: "fixture:fixture-history:1",
          name: "other-fixture-started",
          schemaVersion: 1,
          aggregate: {
            type: "context/other-aggregate",
            id: "aggregate-2",
          },
          streamVersion: 1,
          payload: {},
        },
      ],
    }
    const layout = layoutMessageGraph(expectedGraph(definition, fixture))
    const commandOutcome = {
      responseMessageId: "subject-response",
      commandMessageId: "subject-command-message",
      correlationId: "fixture-topology-correlation",
      observationOrder: 2,
      outcome: { status: "accepted", value: null },
    }
    const operation = {
      operationId: "fixture-topology-operation",
      correlationId: "fixture-topology-correlation",
      operationEventsHref: "/operations/fixture-topology-operation/events",
      correlationEventsHref:
        "/correlations/fixture-topology-correlation/events",
      messageSeriesHref:
        "/operations/fixture-topology-operation/message-series",
      events: {
        kind: "observed",
        href: "/correlations/fixture-topology-correlation/events",
      },
      mode: "test",
      status: "completed",
      context: "context",
      command: "subject-command",
      schemaVersion: 1,
      latestEventId: 2,
      result: { decision: "accepted" },
    }
    const completed = reportGraph(
      {
        runId: "fixture-topology-run",
        testId: definition.id,
        revision: "fixture-topology-revision",
        status: "passed",
        expected: definition.expected,
        observed: {
          messages: [
            {
              kind: "command",
              messageId: "subject-command-message",
              correlationId: "fixture-topology-correlation",
              observationOrder: 1,
              name: "subject-command",
              schemaVersion: 2,
              context: "context",
              payload: {},
            },
          ],
          commandOutcomes: [commandOutcome],
        },
        comparison: {
          status: "passed",
          matches: [
            {
              expectedKey: "subject",
              observedMessageId: "subject-command-message",
            },
          ],
          diagnostics: [],
        },
        commandOutcome,
        operationId: operation.operationId,
        correlationId: operation.correlationId,
        operationHref: "/operations/fixture-topology-operation",
        operationEventsHref: operation.operationEventsHref,
        correlationEventsHref: operation.correlationEventsHref,
        operation,
      },
      fixture
    )
    const completedCommand = completed.find(
      (node) => node.id === "subject-command-message"
    )
    return {
      fixtureNames: layout.nodes
        .filter((node) => node.context === "fixture")
        .map((node) => node.name),
      fixtureParents: layout.nodes
        .filter((node) => node.context === "fixture")
        .map((node) => node.parentId ?? null),
      streamEdges: layout.edges
        .filter((edge) => edge.relationship === "stream-order")
        .map((edge) => [edge.source.id, edge.target.id]),
      completedCommand: {
        status: completedCommand?.status,
        responseStatus: completedCommand?.response?.status,
        parentId: completedCommand?.parentId,
      },
    }
  })
  assert.deepEqual(
    fixtureTopology.fixtureNames,
    ["fixture-started", "fixture-continued", "other-fixture-started"],
    "fixture streams should render their recorded domain events, not a placeholder"
  )
  assert.deepEqual(
    fixtureTopology.fixtureParents,
    [null, "fixture-event-0-fixture-one", null],
    "only events in the same aggregate stream should be ordered together"
  )
  assert.deepEqual(
    fixtureTopology.streamEdges,
    [["fixture-event-0-fixture-one", "fixture-event-0-fixture-two"]],
    "adjacent fixture events should use a stream-order relationship"
  )
  assert.deepEqual(
    fixtureTopology.completedCommand,
    {
      status: "accepted",
      responseStatus: "accepted",
      parentId: "fixture-event-0-fixture-two",
    },
    "completed rendering should use the current report outcome and operation contracts"
  )
  assert.equal(
    fixturePresentation.commandIncomingEdges,
    1,
    "the subject command should connect to the fixture state it evaluates"
  )
  assert.equal(
    fixturePresentation.leadLines,
    0,
    "the decorative command lead should be removed"
  )
  assert.ok(
    fixturePresentation.arrows.every(
      ({ markerEnd, markerId, orientation, relationship }) =>
        relationship === "causation"
          ? markerEnd === `url(#${markerId})` && orientation === "auto"
          : !markerEnd && !markerId
    ),
    "only causal edges should use endpoint-aligned SVG markers"
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
  const commandFocus = await page.evaluate(() => {
    const graph = document
      .querySelector(".message-graph")
      .getBoundingClientRect()
    const command = document
      .querySelector('[data-node-id="command-rent"] .message-node')
      .getBoundingClientRect()
    return {
      x: Math.abs(
        command.left + command.width / 2 - (graph.left + graph.width / 2)
      ),
      y: Math.abs(
        command.top + command.height / 2 - (graph.top + graph.height / 2)
      ),
      zoom: Number(
        document.querySelector(".graph-zoom-value").textContent.replace("%", "")
      ),
    }
  })
  assert.ok(
    commandFocus.x <= 1 && commandFocus.y <= 1,
    `the subject command should own the initial viewport focus: ${JSON.stringify(commandFocus)}`
  )
  assert.equal(commandFocus.zoom, 100, "command focus should use 100% zoom")
  await new Promise((resolve) => setTimeout(resolve, 700))
  const fixtureScreenshot = path.join(
    tmpdir(),
    "rostfrei-tracer-studio-fixture-flow.png"
  )
  await page.screenshot({ path: fixtureScreenshot })
  await page.evaluate(() => {
    window.__edgeAnimationStarts = 0
    window.__nodeAnimationStarts = 0
    document.querySelectorAll("[data-graph-edge]").forEach((edge, index) => {
      edge.dataset.stabilityMarker = `edge-${index}`
    })
    document.querySelectorAll("[data-graph-node]").forEach((node, index) => {
      node.dataset.stabilityMarker = `node-${index}`
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
        if (
          event.target instanceof Element &&
          event.target.classList.contains("message-flow-node")
        ) {
          window.__nodeAnimationStarts += 1
        }
      })
  })

  const testsPanelBeforeDrag = await page.$eval(
    "#studio-tests-panel",
    (panel) => {
      const bounds = panel.getBoundingClientRect()
      return { x: bounds.x, y: bounds.y }
    }
  )
  const testsPanelHeader = await page.$(
    "#studio-tests-panel .studio-panel-header"
  )
  const testsPanelHeaderBounds = await testsPanelHeader.boundingBox()
  assert.ok(
    testsPanelHeaderBounds,
    "the Tests panel header should be draggable"
  )
  await page.mouse.move(
    testsPanelHeaderBounds.x + testsPanelHeaderBounds.width / 2,
    testsPanelHeaderBounds.y + testsPanelHeaderBounds.height / 2
  )
  await page.mouse.down()
  await page.mouse.move(
    testsPanelHeaderBounds.x + testsPanelHeaderBounds.width / 2 + 90,
    testsPanelHeaderBounds.y + testsPanelHeaderBounds.height / 2 + 35,
    { steps: 8 }
  )
  await page.mouse.up()
  const testsPanelAfterDrag = await page.$eval(
    "#studio-tests-panel",
    (panel) => {
      const bounds = panel.getBoundingClientRect()
      return { x: bounds.x, y: bounds.y }
    }
  )
  assert.ok(
    testsPanelAfterDrag.x > testsPanelBeforeDrag.x + 70 &&
      testsPanelAfterDrag.y > testsPanelBeforeDrag.y + 20,
    "dragging the Tests header should reposition its glass panel"
  )

  await page.click('button[aria-controls="studio-tests-panel"]')
  await page.waitForFunction(
    () =>
      document
        .querySelector('button[aria-controls="studio-tests-panel"]')
        ?.getAttribute("aria-expanded") === "false"
  )
  await page.keyboard.down("Control")
  await page.keyboard.down("Shift")
  await page.keyboard.press("E")
  await page.keyboard.up("Shift")
  await page.keyboard.up("Control")
  await page.waitForFunction(
    () =>
      document
        .querySelector('button[aria-controls="studio-tests-panel"]')
        ?.getAttribute("aria-expanded") === "true"
  )

  await page.keyboard.down("Control")
  await page.keyboard.down("Shift")
  await page.keyboard.press("Y")
  await page.keyboard.up("Shift")
  await page.keyboard.up("Control")
  await page.waitForFunction(
    () =>
      document
        .querySelector('button[aria-controls="studio-runs-panel"]')
        ?.getAttribute("aria-expanded") === "true"
  )
  await page.click('button[aria-controls="studio-runs-panel"]')

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

  for (let index = 0; index < 2; index += 1) {
    await page.mouse.move(
      paneBounds.x + paneBounds.width - 80,
      paneBounds.y + 120
    )
    await page.mouse.down()
    await page.mouse.move(paneBounds.x + 80, paneBounds.y + 120, { steps: 12 })
    await page.mouse.up()
  }
  await new Promise((resolve) => setTimeout(resolve, 250))
  const offscreenGraph = await page.evaluate(() => ({
    nodes: [...document.querySelectorAll("[data-graph-node]")].map(
      (node) => node.dataset.stabilityMarker
    ),
    edges: [...document.querySelectorAll("[data-graph-edge]")].map(
      (edge) => edge.dataset.stabilityMarker
    ),
    visibleNodes: [...document.querySelectorAll("[data-graph-node]")].filter(
      (node) => {
        const bounds = node.getBoundingClientRect()
        return bounds.right >= 0 && bounds.left <= innerWidth
      }
    ).length,
  }))
  assert.equal(
    offscreenGraph.visibleNodes,
    0,
    "the persistence check should pan every node offscreen"
  )
  assert.deepEqual(
    offscreenGraph.nodes,
    Array.from({ length: 5 }, (_, index) => `node-${index}`),
    "panning offscreen should not unmount message nodes"
  )
  assert.deepEqual(
    offscreenGraph.edges,
    Array.from({ length: 4 }, (_, index) => `edge-${index}`),
    "panning offscreen should not unmount message edges"
  )
  const viewportOffscreen = await page.$eval(
    ".react-flow__viewport",
    (element) => element.style.transform
  )
  await page.click('button[aria-label="Fit graph to view"]')
  await page.waitForFunction(
    (previous) =>
      document.querySelector(".react-flow__viewport")?.style.transform !==
      previous,
    {},
    viewportOffscreen
  )
  await new Promise((resolve) => setTimeout(resolve, 500))

  const branchEvent = await page.$(
    '[data-node-id="event-rented"] .message-node'
  )
  assert.ok(branchEvent, "a domain event should be available to inspect")
  await branchEvent.hover()
  await new Promise((resolve) => setTimeout(resolve, 250))
  assert.equal(
    await page.$("[data-node-popup]"),
    null,
    "message details should not open on hover"
  )
  await page.click('[data-node-id="event-rented"] .message-node')
  await page.waitForSelector("[data-node-popup]", { visible: true })
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
    "clicking a message should open persistent details"
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
  await page.mouse.click(
    paneBounds.x + paneBounds.width - 24,
    paneBounds.y + 80
  )
  await page.waitForSelector("[data-node-popup]", { hidden: true })

  const commandNode = await page.$(
    '[data-node-id="command-rent"] .message-node'
  )
  assert.ok(commandNode, "the command node should be available to click")
  await commandNode.click()
  await page.waitForSelector("[data-command-response]", { visible: true })
  assert.equal(
    await page.$$eval("[data-node-popup]", (popups) => popups.length),
    1,
    "only one message detail popup should be open"
  )
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
  await page.mouse.click(
    paneBounds.x + paneBounds.width - 24,
    paneBounds.y + 80
  )
  await page.waitForSelector("[data-node-popup]", { hidden: true })
  const edgeStability = await page.evaluate(() => ({
    animationStarts: window.__edgeAnimationStarts,
    nodeAnimationStarts: window.__nodeAnimationStarts,
    markers: [...document.querySelectorAll("[data-graph-edge]")].map(
      (edge) => edge.dataset.stabilityMarker
    ),
    nodeMarkers: [...document.querySelectorAll("[data-graph-node]")].map(
      (node) => node.dataset.stabilityMarker
    ),
  }))
  assert.equal(
    edgeStability.animationStarts,
    0,
    "graph interactions should not restart edge animations"
  )
  assert.deepEqual(
    edgeStability.markers,
    ["edge-0", "edge-1", "edge-2", "edge-3"],
    "graph interactions should preserve the existing edge elements"
  )
  assert.equal(
    edgeStability.nodeAnimationStarts,
    0,
    "panning should not replay node arrival animations"
  )
  assert.deepEqual(
    edgeStability.nodeMarkers,
    Array.from({ length: 5 }, (_, index) => `node-${index}`),
    "graph interactions should preserve the existing message nodes"
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
  await page.click('button[aria-controls="studio-runs-panel"]')
  await page.waitForFunction(
    () =>
      document
        .querySelector('button[aria-controls="studio-runs-panel"]')
        ?.getAttribute("aria-expanded") === "true"
  )
  const runScroll = await page.$eval(".past-runs-scroll", (element) => {
    const style = getComputedStyle(element)
    const sidebarStyle = getComputedStyle(
      document.querySelector(".studio-runs-panel")
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
  await page.mouse.move(300, 100)
  await new Promise((resolve) => setTimeout(resolve, 250))
  assert.ok(
    await page.$("[data-command-response]"),
    "command details should remain open when the pointer leaves"
  )
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
      document.querySelectorAll("[data-graph-node][data-context]").length === 3
  )
  const setupContext = await page.evaluate(() => ({
    ids: [...document.querySelectorAll("[data-graph-node][data-context]")].map(
      (node) => node.dataset.nodeId
    ),
    subjectIncomingEdges: document.querySelectorAll(
      '[data-graph-edge][data-target-id="expected-return-rented-bicycle-subject"]'
    ).length,
    mutedEdges: document.querySelectorAll(".graph-edge-context").length,
    contextArrowheads: document.querySelectorAll(
      '[data-edge-relationship="context"] marker'
    ).length,
  }))
  assert.deepEqual(
    setupContext.ids,
    [
      "fixture-event-0-rented-demo-bike-42-added",
      "fixture-event-0-rented-demo-bike-99-added",
      "fixture-event-0-fixture-bike-42-rented",
    ],
    "the canonical fixture history should appear as subdued graph context"
  )
  assert.equal(
    setupContext.subjectIncomingEdges,
    1,
    "the subject command should connect to its fixture state"
  )
  assert.equal(
    setupContext.mutedEdges,
    3,
    "fixture stream ordering and the subject link should use the muted context style"
  )
  assert.equal(
    setupContext.contextArrowheads,
    0,
    "fixture context should not be presented as message causation"
  )

  await page.evaluate(() => {
    const button = [...document.querySelectorAll(".test-row button")].find(
      (candidate) => candidate.textContent?.includes("Reject a maintenance")
    )
    button.click()
  })
  await page.waitForSelector(
    '[data-node-id="expected-reject-unavailable-bicycle-subject"]'
  )
  await page.click('button[aria-label^="Run "]')
  await page.waitForFunction(
    () =>
      document.querySelector(
        '[data-node-id="expected-reject-unavailable-bicycle-subject"]'
      )?.dataset.status === "rejected"
  )
  await page.click(
    '[data-node-id="expected-reject-unavailable-bicycle-subject"] .message-node'
  )
  await page.waitForSelector("[data-command-response]", { visible: true })
  await new Promise((resolve) => setTimeout(resolve, 300))
  const rejectionPopup = await page.$eval(
    "[data-command-response]",
    (element) => {
      const payload = element.querySelector(".payload-list")
      const style = getComputedStyle(payload)
      const popupStyle = getComputedStyle(element.closest("[data-node-popup]"))
      const scrollbar = getComputedStyle(payload, "::-webkit-scrollbar")
      const track = getComputedStyle(payload, "::-webkit-scrollbar-track")
      return {
        text: element.textContent,
        overflowX: style.overflowX,
        overflowY: style.overflowY,
        overscrollBehavior: style.overscrollBehavior,
        scrollbarColor: style.scrollbarColor,
        scrollbarWidth: scrollbar.width,
        trackBackground: track.backgroundColor,
        popupBorderColor: popupStyle.borderColor,
        responsePaths: [...element.querySelectorAll(".payload-row dt")].map(
          (term) => term.textContent
        ),
      }
    }
  )
  assert.ok(
    rejectionPopup.text?.includes("BICYCLE_UNAVAILABLE"),
    "the rejected command response should expose the business rejection"
  )
  assert.ok(
    rejectionPopup.responsePaths.includes("details.bicycle_id"),
    "unique business rejection details should remain visible"
  )
  assert.equal(
    rejectionPopup.responsePaths.includes("details.code"),
    false,
    "a detail code duplicated by the canonical rejection code should be collapsed"
  )
  assert.equal(
    rejectionPopup.responsePaths.includes("details.message"),
    false,
    "a detail message duplicated by the canonical rejection message should be collapsed"
  )
  assert.equal(
    rejectionPopup.overflowX,
    "hidden",
    "popup payloads should not show a horizontal scrollbar"
  )
  assert.equal(rejectionPopup.overflowY, "auto")
  assert.equal(rejectionPopup.overscrollBehavior, "contain")
  assert.notEqual(
    rejectionPopup.scrollbarColor,
    "auto",
    "popup payloads should inherit the global themed scrollbar"
  )
  assert.equal(rejectionPopup.scrollbarWidth, "6px")
  assert.equal(rejectionPopup.trackBackground, "rgba(0, 0, 0, 0)")
  assert.match(
    rejectionPopup.popupBorderColor,
    /rgba?\(235, 111, 126/,
    "rejected commands should receive a restrained danger treatment"
  )
  const rejectionScreenshot = path.join(
    tmpdir(),
    "rostfrei-tracer-studio-rejection.png"
  )
  await page.screenshot({ path: rejectionScreenshot })

  await page.keyboard.press("Escape")
  await page.click('button[aria-controls="studio-command-panel"]')
  await page.waitForSelector("#studio-command-panel select:not(:disabled)")
  assert.equal(
    await page.$eval('[data-command-mode="test"]', (button) => button.disabled),
    false,
    "Test mode should be selectable before choosing a supported command"
  )
  assert.equal(
    await page.$eval('[data-command-mode="test"]', (button) =>
      button.getAttribute("aria-pressed")
    ),
    "true",
    "Test mode should be the default command execution path"
  )
  const panelLayers = await page.evaluate(() => ({
    command: Number(
      getComputedStyle(document.querySelector("#studio-command-panel")).zIndex
    ),
    runs: Number(
      getComputedStyle(document.querySelector("#studio-runs-panel")).zIndex
    ),
  }))
  assert.ok(
    panelLayers.command > panelLayers.runs,
    "a newly opened Command panel should move in front of an open panel"
  )
  const commandChoice = await page.$eval(
    "#studio-command-panel select",
    (select) =>
      [...select.options].find((option) =>
        option.textContent?.includes("Rent bicycle")
      )?.value
  )
  assert.ok(commandChoice, "the Catalog command should be available")
  await page.select("#studio-command-panel select", commandChoice)
  await page.waitForFunction(
    () =>
      document.querySelector('input[aria-label="Payload Fleet"]')?.value ===
        "city-fleet" &&
      document.querySelector('input[aria-label="Payload Bicycle"]')?.value ===
        "bike-42"
  )
  const generatedRequestId = await page.$eval(
    'input[aria-label="Payload Request id"]',
    (input) => input.value
  )
  assert.match(
    generatedRequestId,
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
    "an ID without advertised values should be prefilled with a UUID"
  )
  assert.equal(
    await page.$(
      '#studio-command-panel textarea[aria-label="Payload Request id"]'
    ),
    null,
    "opaque IDs should use normal text fields instead of JSON textareas"
  )
  assert.equal(
    await page.$eval(
      '#studio-command-panel [data-payload-field="request_id"]',
      (field) => field.textContent?.includes("JSON value")
    ),
    false,
    "identifier fields should not ask users for raw JSON"
  )
  assert.equal(
    await page.$('#studio-command-panel textarea[aria-label="Payload JSON"]'),
    null,
    "the command payload should use catalog-driven form controls"
  )
  await page.select(
    '#studio-command-panel select[aria-label="Payload Attempt"]',
    "option:0"
  )
  await page.click('[data-command-mode="preview"]')
  await page.click('#studio-command-panel button[type="submit"]')
  await page.waitForSelector("[data-command-result]")
  await page.waitForFunction(
    () =>
      document
        .querySelector("[data-command-result]")
        ?.textContent?.includes("accepted") &&
      document.querySelectorAll("[data-graph-node]").length === 4 &&
      document.querySelectorAll("[data-graph-edge]").length === 2
  )
  assert.deepEqual(
    previewRequests,
    [
      {
        body: `{"schemaVersion":2,"payload":{"fleet_id":"city-fleet","bicycle_id":"bike-42","request_id":"${generatedRequestId}","attempt":9007199254740993}}`,
        idempotencyKey: undefined,
        authorization: "Bearer local-development-token",
      },
    ],
    "Preview should use the control token and submit exactly once without an idempotency key or rounded numeric options"
  )
  const commandPreview = await page.$eval(
    "[data-command-result]",
    (result) => ({
      text: result.textContent,
      capture: result.querySelector(".command-result-meta")?.textContent,
    })
  )
  assert.ok(
    commandPreview.text?.includes("rent-bicycle v2"),
    "the command pane should identify the completed operation"
  )
  assert.ok(
    commandPreview.capture?.includes("grouped"),
    "the command pane should report message-series fidelity"
  )
  assert.equal(
    await page.$eval(
      '[data-node-id="studio-command"]',
      (node) => node.dataset.status
    ),
    "accepted",
    "the Preview result should replace the canvas with the returned message series"
  )
  assert.equal(
    await page.$eval(
      '[data-node-id="studio-downstream-command"]',
      (node) => node.dataset.status
    ),
    "idle",
    "a command without an observed outcome must not be presented as accepted"
  )
  assert.equal(
    await page.$$eval(
      '[data-graph-edge][data-target-id="studio-unlinked-event"]',
      (edges) => edges.length
    ),
    0,
    "grouped messages without explicit causation must remain disconnected"
  )
  const commandScreenshot = path.join(
    tmpdir(),
    "rostfrei-tracer-studio-command-form.png"
  )
  await page.screenshot({ path: commandScreenshot })

  await page.click('[data-command-mode="test"]')
  const testIdempotencyKey = await page.$eval(
    'input[aria-label="Idempotency key"]',
    (input) => input.value
  )
  assert.match(
    testIdempotencyKey,
    /^studio:test:[A-Za-z0-9-]+$/,
    "Test publication should start with a valid stable idempotency key"
  )
  await page.$eval('input[aria-label="Idempotency key"]', (input) => {
    const setValue = Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value"
    ).set
    setValue.call(input, "invalid key")
    input.dispatchEvent(new Event("input", { bubbles: true }))
  })
  await page.click('#studio-command-panel button[type="submit"]')
  await page.waitForFunction(() =>
    document
      .querySelector(".command-form-error")
      ?.textContent?.includes("Idempotency key must be")
  )
  assert.equal(
    await page.$eval('button[aria-controls="studio-command-panel"]', (button) =>
      button.getAttribute("aria-expanded")
    ),
    "true",
    "validation errors should keep the Command panel open"
  )
  assert.equal(testRequests.length, 0)
  await page.$eval(
    'input[aria-label="Idempotency key"]',
    (input, value) => {
      const setValue = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value"
      ).set
      setValue.call(input, value)
      input.dispatchEvent(new Event("input", { bubbles: true }))
    },
    testIdempotencyKey
  )
  const ambiguousTest = await page.evaluate(async () => {
    const { submitCommand } = await import("/src/lib/api.ts")
    try {
      await submitCommand(
        "test",
        "/ambiguous/city-fleet/test",
        1,
        "{}",
        "studio:test:ambiguous"
      )
      return { name: "", message: "" }
    } catch (error) {
      return {
        name: error instanceof Error ? error.name : "",
        message: error instanceof Error ? error.message : "",
      }
    }
  })
  assert.deepEqual(ambiguousTestRequests, [
    {
      body: '{"schemaVersion":1,"payload":{}}',
      idempotencyKey: "studio:test:ambiguous",
    },
  ])
  assert.equal(ambiguousTest.name, "CommandSubmissionIndeterminateError")
  assert.ok(
    ambiguousTest.message.includes("may have reached the bus") &&
      ambiguousTest.message.includes("same idempotency key"),
    "an accepted Test response without an operation Location must remain indeterminate"
  )
  await page.click('#studio-command-panel button[type="submit"]')
  await page.waitForFunction(
    () =>
      document
        .querySelector('button[aria-controls="studio-command-panel"]')
        ?.getAttribute("aria-expanded") === "false" &&
      document.querySelector('[data-node-id="studio-test-command"]')?.dataset
        .status === "accepted",
    { timeout: 30000 }
  )
  assert.deepEqual(
    testRequests,
    [
      {
        body: `{"schemaVersion":2,"payload":{"fleet_id":"city-fleet","bicycle_id":"bike-42","request_id":"${generatedRequestId}","attempt":9007199254740993}}`,
        idempotencyKey: testIdempotencyKey,
        authorization: "Bearer local-development-token",
      },
    ],
    "a Test push should close the panel and send the exact body once"
  )
  assert.deepEqual(
    testSeriesRequests,
    ["?within=10s&settleFor=500ms"],
    "Test inspection should follow the advertised message-series href with bounded settling"
  )
  await page.click('button[aria-controls="studio-command-panel"]')
  await page.waitForFunction(
    () =>
      document
        .querySelector('button[aria-controls="studio-command-panel"]')
        ?.getAttribute("aria-expanded") === "true"
  )
  await page.waitForFunction(
    (previousKey) =>
      document.querySelector('input[aria-label="Idempotency key"]')?.value !==
      previousKey,
    {},
    testIdempotencyKey
  )
  const rotatedTestKey = await page.$eval(
    'input[aria-label="Idempotency key"]',
    (input) => input.value
  )
  await page.waitForFunction(
    (requestId) =>
      document.querySelector('input[aria-label="Payload Fleet"]')?.value ===
        "city-fleet" &&
      document.querySelector('input[aria-label="Payload Bicycle"]')?.value ===
        "bike-42" &&
      document.querySelector('input[aria-label="Payload Request id"]')
        ?.value === requestId,
    {},
    generatedRequestId
  )
  await page.select(
    '#studio-command-panel select[aria-label="Payload Attempt"]',
    "option:0"
  )
  await page.click('#studio-command-panel button[type="submit"]')
  await page.waitForFunction(
    () =>
      document
        .querySelector('button[aria-controls="studio-command-panel"]')
        ?.getAttribute("aria-expanded") === "false" &&
      document.querySelector('[data-graph-node][data-status="indeterminate"]'),
    { timeout: 30000 }
  )
  await page.click('button[aria-controls="studio-command-panel"]')
  await page.waitForFunction(() =>
    document
      .querySelector(".command-form-error")
      ?.textContent?.includes("may have reached the bus")
  )
  assert.equal(
    await page.$eval(
      'input[aria-label="Idempotency key"]',
      (input) => input.value
    ),
    rotatedTestKey,
    "an ambiguous Test publication must preserve its submitted idempotency key"
  )
  assert.deepEqual(
    await page.$$eval(
      '#studio-command-panel select[aria-label^="Payload "]',
      (selects) => selects.map((select) => select.value)
    ),
    ["option:0"],
    "an ambiguous Test publication must preserve the exact retry payload"
  )
  assert.deepEqual(
    await page.$$eval(
      '#studio-command-panel input[aria-label^="Payload "]',
      (inputs) => inputs.map((input) => input.value)
    ),
    ["city-fleet", "bike-42", generatedRequestId],
    "an ambiguous Test publication must preserve prefilled identifiers"
  )
  assert.deepEqual(testRequests[1], {
    body: `{"schemaVersion":2,"payload":{"fleet_id":"city-fleet","bicycle_id":"bike-42","request_id":"${generatedRequestId}","attempt":9007199254740993}}`,
    idempotencyKey: rotatedTestKey,
    authorization: "Bearer local-development-token",
  })

  await page.setViewport({ width: 390, height: 844, deviceScaleFactor: 1 })
  await page.reload({ waitUntil: "networkidle0" })
  await page.waitForSelector("[data-graph-node]")
  await page.waitForFunction(() => {
    const graph = document
      .querySelector(".message-graph")
      ?.getBoundingClientRect()
    const command = document
      .querySelector(
        "[data-graph-node]:not([data-context]) .message-node-command"
      )
      ?.getBoundingClientRect()
    return (
      graph &&
      command &&
      Math.abs(
        command.left + command.width / 2 - (graph.left + graph.width / 2)
      ) <= 1
    )
  })
  const mobileFocus = await page.evaluate(() => {
    const graph = document
      .querySelector(".message-graph")
      .getBoundingClientRect()
    const command = document
      .querySelector(
        "[data-graph-node]:not([data-context]) .message-node-command"
      )
      .getBoundingClientRect()
    const fixtureNodes = [
      ...document.querySelectorAll("[data-graph-node][data-context=fixture]"),
    ]
    return {
      commandCenterDelta: Math.max(
        Math.abs(
          command.left + command.width / 2 - (graph.left + graph.width / 2)
        ),
        Math.abs(
          command.top + command.height / 2 - (graph.top + graph.height / 2)
        )
      ),
      fixtureNodes: fixtureNodes.length,
      offscreenFixtureNodes: fixtureNodes.filter((node) => {
        const bounds = node.getBoundingClientRect()
        return (
          bounds.x < 0 ||
          bounds.right > innerWidth ||
          bounds.y < 0 ||
          bounds.bottom > innerHeight
        )
      }).length,
    }
  })
  assert.ok(
    mobileFocus.commandCenterDelta <= 1,
    `the command should remain the mobile viewport focus: ${JSON.stringify(mobileFocus)}`
  )
  assert.equal(
    mobileFocus.fixtureNodes,
    2,
    "the canonical fixture events should remain mounted for looking back"
  )
  assert.ok(
    mobileFocus.offscreenFixtureNodes > 0,
    "fixture context may sit offscreen behind the focused command"
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
      nodeAnimationRestarts: edgeStability.nodeAnimationStarts,
      pastRuns: 16,
      fixtureScreenshot,
      screenshot,
      rejectionScreenshot,
      commandScreenshot,
      commandPreview: true,
      commandTest: true,
      commandIndeterminate: true,
    })
  )
} finally {
  await browser?.close()
  server.kill("SIGTERM")
}

function operationSnapshot(status, mode = "simulate") {
  const test = mode === "test"
  const operationId = test ? "studio-test" : "studio-preview"
  const correlationId = test ? "studio-test-correlation" : "studio-correlation"
  return {
    operationId,
    correlationId,
    operationEventsHref: `/operations/${operationId}/events`,
    correlationEventsHref: `/correlations/${correlationId}/events`,
    messageSeriesHref: `/operations/${operationId}/message-series`,
    events: {
      kind: test ? "observed" : "predicted",
      href: test
        ? `/correlations/${correlationId}/events`
        : `/operations/${operationId}/events`,
    },
    mode,
    status,
    command: "rent-bicycle",
    schemaVersion: 2,
    context: "bike-rental",
    latestEventId: status === "completed" ? 5 : 1,
    ...(status === "completed"
      ? {
          result: test
            ? {
                decision: "accepted",
                commandMessageId: "studio-test-command",
                responseMessageId: "studio-test-response",
                published: true,
                duplicate: false,
              }
            : {
                decision: "accepted",
                baseStreamVersion: 1,
                predictedEvents: [
                  {
                    ordinal: 0,
                    predictedStreamVersion: 2,
                    eventType: "bicycle-rented",
                    schemaVersion: 1,
                    payload: { bicycle_id: "bike-42" },
                  },
                ],
                published: false,
              },
        }
      : {}),
  }
}

function testMessageSeries() {
  return {
    operationId: "studio-test",
    correlationId: "studio-test-correlation",
    mode: "test",
    messageSeries: {
      messages: [
        {
          kind: "command",
          messageId: "studio-test-command",
          correlationId: "studio-test-correlation",
          observationOrder: 0,
          name: "rent-bicycle",
          schemaVersion: 2,
          context: "bike-rental",
          payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
        },
        {
          kind: "domain-event",
          messageId: "studio-test-event",
          correlationId: "studio-test-correlation",
          causationId: "studio-test-command",
          observationOrder: 1,
          name: "bicycle-rented",
          schemaVersion: 1,
          aggregate: {
            type: "bike-rental/rental-fleet",
            id: "city-fleet",
          },
          payload: { bicycle_id: "bike-42" },
        },
      ],
      commandOutcomes: [
        {
          responseMessageId: "studio-test-response",
          commandMessageId: "studio-test-command",
          correlationId: "studio-test-correlation",
          observationOrder: 2,
          outcome: { status: "accepted", value: null },
        },
      ],
    },
    capture: {
      settled: true,
      settledFor: "500ms",
      fidelity: "exact",
    },
  }
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
