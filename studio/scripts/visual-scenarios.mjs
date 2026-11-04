import assert from "node:assert/strict"
import { historicalCapability, historicalMessages } from "./historical-fixture.mjs"
import { snapshotCapability, snapshotHistory, snapshotReconstruction } from "./snapshot-fixture.mjs"
import { timelineCapability, timelineMessages, timelinePage, timelineReconstruction } from "./timeline-fixture.mjs"

import { settlePage } from "./browser.mjs"
import {
  observationCapability,
  observationFlow,
  observationSnapshot,
} from "./observation-fixture.mjs"

export const scenarios = {
  overview: "Expected rental flow in the Workbench layout",
  command: "Populated catalog-driven Command panel",
  handler: "Uncommitted handler decision and proposed domain events",
  "handler-rejected": "Rejected handler diagnostic with no proposed events",
  "handler-empty": "Accepted handler diagnostic with no proposed events",
  accepted: "Accepted Test with command details open",
  rejected: "Business rejection with command details open",
  loading: "Pending Test with observing indicator",
  indeterminate: "Unconfirmed Test submission with retry details",
  branching: "A 25-message synthetic branching flow after Fit graph to view",
  "long-payload": "Nested, long payload values in the message inspector",
  narrow: "Accepted command details at 390 × 844",
  observation: "Externally initiated flow in the continuous Observe panel",
  historical: "Persisted aggregate history and a response-backed historical flow",
  snapshots: "Stored snapshot, reconstructed state and optional snapshot-assisted replay",
  timeline: "Historical timeline paused after a command outcome and integration event",
  "timeline-aggregate": "Aggregate state follows the historical timeline playhead",
}

const commandPath = "/contexts/bike-rental/commands/rent-bicycle"
const operationPath = "/operations/visual-inspection"
const payload = { fleet_id: "city-fleet", bicycle_id: "bike-42" }
const catalog = {
  catalogVersion: 1,
  testRepository: { definitionsHref: "/behavior-tests" },
  contexts: [
    {
      id: "bike-rental",
      label: "Visual fixtures",
      aggregates: [],
      commands: [
        {
          id: "rent-bicycle",
          label: "Visual fixture: Rent bicycle",
          versions: [
            {
              schemaVersion: 2,
              contentType: "application/json",
              fields: Object.keys(payload).map((name) => ({
                name,
                value: { kind: "opaque" },
              })),
              payloadTemplate: { fleet_id: null, bicycle_id: null },
              testInputsHrefTemplate: `${commandPath}/schemas/2/inputs`,
              testHrefTemplate: `${commandPath}/test`,
              simulateHandlerHrefTemplate: `${commandPath}/simulate-handler`,
            },
          ],
        },
      ],
    },
  ],
}

export async function installScenario(page, scene, samples, diagnostics) {
  if (scene === "observation") {
    await page.evaluateOnNewDocument((snapshot) => {
      const fetch = window.fetch.bind(window)
      window.fetch = (input, options) => {
        if (
          new URL(String(input), location.origin).pathname ===
          "/api/observation/test/events"
        ) {
          const body = new ReadableStream({
            start(controller) {
              controller.enqueue(
                new TextEncoder().encode(
                  `event: observation\ndata: ${JSON.stringify(snapshot)}\n\n`
                )
              )
            },
          })
          return Promise.resolve(
            new Response(body, {
              headers: { "content-type": "text/event-stream" },
            })
          )
        }
        return fetch(input, options)
      }
    }, observationSnapshot())
  }
  if (scene === "indeterminate") {
    diagnostics.expectedHttpErrors.push({
      method: "POST",
      path: `/api${commandPath}/test`,
      status: 503,
    })
  }
  // Freeze data identities and wall time, keeping timers and performance.now real
  // so the application's asynchronous work and React Flow can still progress.
  await page.evaluateOnNewDocument(() => {
    const NativeDate = Date
    const instant = NativeDate.parse("2026-09-10T12:00:00.000Z")
    window.Date = class extends NativeDate {
      constructor(...args) {
        super(...(args.length ? args : [instant]))
      }
      static now() {
        return instant
      }
    }
    let identity = 0
    crypto.randomUUID = () =>
      `00000000-0000-4000-8000-${String(++identity).padStart(12, "0")}`
  })
  const tests = samples.SAMPLE_TESTS.map((test) => ({
    ...test,
    name: `Visual fixture: ${test.name}`,
  }))
  const series = messageSeries(scene)
  await page.setRequestInterception(true)
  page.on("request", (request) => {
    // Chrome probes this optional icon even though index.html declares none.
    if (new URL(request.url()).pathname === "/favicon.ico") {
      void request.respond({ status: 204 })
      return
    }
    // The inspection server fixes API_BASE to /api for fixture sessions.
    // An unhandled request is a fixture failure, never a live API fallback.
    if (!["fetch", "xhr"].includes(request.resourceType())) {
      void request.continue()
      return
    }
    const pathname = new URL(request.url()).pathname.replace(/^\/api(?=\/)/, "")
    const method = request.method()
    let body
    let status = 200
    let headers = {}
    if (method === "GET" && pathname === "/catalog")
      body =
        scene === "observation"
          ? { ...catalog, observation: [observationCapability] }
          : scene === "snapshots" ? { ...catalog, historical: [snapshotCapability] }
           : ["historical", "timeline", "timeline-aggregate"].includes(scene) ? { ...catalog, historical: [timelineCapability] } : catalog
    else if (["timeline", "timeline-aggregate"].includes(scene) && pathname === timelineCapability.messagesHref)
      body = timelinePage(scene === "timeline-aggregate" ? timelineMessages.filter(item => item.kind === "domain-event") : timelineMessages)
    else if (scene === "timeline-aggregate" && pathname === timelineCapability.aggregateHref)
      body = timelineReconstruction(new URL(request.url()).searchParams)
    else if (scene === "snapshots" && pathname === snapshotCapability.messagesHref)
      body = snapshotHistory
    else if (scene === "snapshots" && pathname === snapshotCapability.aggregateHref)
      body = snapshotReconstruction()
    else if (scene === "historical" && pathname === historicalCapability.messagesHref)
      body = timelinePage((new URL(request.url()).searchParams.has("aggregateType") ? [historicalMessages[0]] : historicalMessages).map(item => ({ ...item, storedAtUnixNanos: "1790848800000000000" })))
    else if (
      scene === "observation" &&
      pathname === observationFlow().detailHref
    )
      body = observationFlow()
    else if (method === "GET" && pathname === "/behavior-tests")
      body = { items: tests }
    else if (method === "GET" && pathname.startsWith("/tests/")) {
      const revision =
        samples.SAMPLE_DEFINITIONS[pathname.slice("/tests/".length)]
      if (revision)
        body = {
          ...revision,
          definition: {
            ...revision.definition,
            name: `Visual fixture: ${revision.definition.name}`,
          },
        }
    } else if (
      method === "GET" &&
      pathname.startsWith("/test-scenario/fixtures/")
    ) {
      body =
        samples.SAMPLE_FIXTURES[
          pathname.slice("/test-scenario/fixtures/".length)
        ]
    } else if (
      method === "GET" &&
      pathname === `${commandPath}/schemas/2/inputs`
    ) {
      body = {
        fields: Object.entries(payload).map(([name, value]) => ({
          name,
          label: name === "fleet_id" ? "Fleet" : "Bicycle",
          options: [{ value, label: value }],
        })),
      }
    } else if (
      method === "POST" &&
      pathname === `${commandPath}/simulate-handler`
    ) {
      body = {
        kind: "handlerSimulation",
        committed: false,
        context: "bike-rental",
        command: "rent-bicycle",
        schemaVersion: 2,
        outcome:
          scene === "handler-rejected"
            ? {
                decision: "rejected",
                rejection: { code: "BICYCLE_UNAVAILABLE" },
              }
            : { decision: "accepted" },
        streams: [
          {
            aggregateType: "bike-rental/rental-fleet",
            aggregateId: "city-fleet",
            baseStreamVersion: 2,
            readGuard: false,
          },
        ],
        proposedEvents:
          scene === "handler"
            ? [
                {
                  aggregateType: "bike-rental/rental-fleet",
                  aggregateId: "city-fleet",
                  eventType: "bicycle-rented",
                  schemaVersion: 1,
                  proposedStreamVersion: 3,
                  payload,
                },
              ]
            : [],
      }
    } else if (
      method === "POST" &&
      pathname === `${commandPath}/test` &&
      scene !== "indeterminate"
    ) {
      status = 202
      headers = { location: operationPath }
      body = operation("queued")
    } else if (
      method === "POST" &&
      pathname === `${commandPath}/test` &&
      scene === "indeterminate"
    ) {
      // Exercise the real client's ambiguous-submission path with a mocked 503.
      status = 503
      body = { message: "Visual fixture: upstream response unavailable" }
    } else if (method === "GET" && pathname === operationPath) {
      body = operation(scene === "loading" ? "running" : "completed", scene)
    } else if (
      method === "GET" &&
      pathname === `${operationPath}/message-series`
    ) {
      body = series
    }
    if (body === undefined) {
      diagnostics.unexpectedRequests.push({ method, path: pathname })
      status = 501
      body = { message: `Missing visual fixture: ${method} ${pathname}` }
    }
    void request.respond({
      status,
      headers,
      contentType: "application/json",
      body: JSON.stringify(body),
    })
  })
}

export async function prepareScenario(page, scene) {
  await page.waitForFunction(() =>
    document
      .querySelector(".execution-header")
      ?.textContent?.includes("Visual fixture:")
  )
  await page.addStyleTag({
    content: `
    *, *::before, *::after {
      animation-duration: 0s !important;
      animation-delay: 0s !important;
      transition-duration: 0s !important;
      caret-color: transparent !important;
    }
  `,
  })
  await settlePage(page)
  if (scene === "overview") {
    await page.click('[data-layout="workbench"]')
    await settlePage(page)
    return
  }
  if (scene === "snapshots") {
    if (page.viewport().width >= 768) await page.click('[data-layout="workbench"]')
    await page.click('[data-studio-mode="historical"]')
    await page.type('[aria-label="Historical aggregate ID"]', "city-fleet")
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-reconstruction-status="hit"]')
    if (page.viewport().width < 1400) await page.click('[aria-label="Collapse sidebar"]')
    await page.click('[aria-label="Inspect snapshot version 4"]')
    await settlePage(page)
    return
  }
  if (["timeline", "timeline-aggregate"].includes(scene)) {
    if (page.viewport().width >= 768) await page.click('[data-layout="workbench"]')
    await page.click('[data-studio-mode="historical"]')
    if (scene === "timeline-aggregate") await page.type('[aria-label="Historical aggregate ID"]', "city-fleet")
    else await page.click('[aria-label="Historical browsing"] button:last-child')
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[aria-label="Historical timeline"]')
    if (page.viewport().width < 1400) await page.click('[aria-label="Collapse sidebar"]')
    await page.focus('[aria-label="Historical timeline"]')
    await page.keyboard.press("Home")
    for (let i = 0; i < (scene === "timeline-aggregate" ? 1 : 4); i++) await page.keyboard.press("ArrowRight")
    if (scene === "timeline-aggregate") await page.waitForFunction(() => document.querySelector(".reconstruction-summary")?.textContent.includes("Version 1"))
    await settlePage(page)
    return
  }
  if (scene === "historical") {
    if (page.viewport().width >= 768) await page.click('[data-layout="workbench"]')
    await page.click('[data-studio-mode="historical"]')
    await page.click('[aria-label="Historical browsing"] button:last-child')
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-message-id="response.1"]')
    if (page.viewport().width < 1400) await page.click('[aria-label="Collapse sidebar"]')
    if (page.viewport().width >= 1101) await page.click('[data-message-id="response.1"]')
    await settlePage(page)
    return
  }
  if (scene === "observation") {
    if (page.viewport().width >= 768)
      await page.click('[data-layout="workbench"]')
    await page.click('[aria-label="Open observation panel"]')
    await page.waitForSelector('[data-observation-status="live"]')
    await page.click('[data-observed-flow="test-external-flow"]')
    await page.waitForSelector('[data-message-id="published"]')
    if (page.viewport().width >= 768)
      await page.waitForFunction(() =>
        [...document.querySelectorAll("[data-graph-node]")].every((node) => {
          const bounds = node.getBoundingClientRect()
          return node.contains(
            document.elementFromPoint(
              bounds.x + bounds.width / 2,
              bounds.y + bounds.height / 2
            )
          )
        })
      )
    return
  }
  await setPanel(page, "tests", false)
  await setPanel(page, "command", true)
  const select = 'select[aria-label="Command and schema"]'
  const value = await page.$eval(select, (element) => element.options[1]?.value)
  assert.ok(value, "The visual catalog must contain a command")
  await page.select(select, value)
  await page.waitForFunction(
    () =>
      document.querySelector('input[aria-label="Payload Fleet"]')?.value ===
        "city-fleet" &&
      document.querySelector('input[aria-label="Payload Bicycle"]')?.value ===
        "bike-42"
  )
  if (scene === "command") return
  if (scene.startsWith("handler")) {
    await page.click('[data-command-mode="simulate-handler"]')
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForSelector("[data-handler-simulation-result]")
    await page.$eval("[data-handler-simulation-result]", (element) =>
      element.scrollIntoView({ block: "end" })
    )
    return
  }
  await page.click('#studio-command-panel button[type="submit"]')

  if (scene === "loading") {
    await page.waitForSelector('[data-graph-node][data-status="running"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".command-execution-header")
        ?.textContent?.includes("Execution in progress")
    )
    await setPanel(page, "command", false)
    return
  }
  if (scene === "indeterminate") {
    await page.waitForSelector('[data-graph-node][data-status="indeterminate"]')
    await setPanel(page, "command", true)
    await page.waitForFunction(() =>
      document
        .querySelector(".command-form-error")
        ?.textContent?.includes("may have reached the bus")
    )
    return
  }
  const expectedCount =
    scene === "branching" ? 25 : scene === "rejected" ? 1 : 3
  await page.waitForFunction(
    (count) =>
      document.querySelectorAll(".message-card").length === count &&
      document.querySelector(
        '[data-message-id="visual-command"] .message-card-footer'
      )?.dataset.status === (count === 1 ? "rejected" : "accepted"),
    {},
    expectedCount
  )
  await setPanel(page, "command", false)
  await settlePage(page)
  if (scene === "branching") {
    await page.click('button[aria-label="Fit graph to view"]')
    return
  }
  await page.click('[data-message-id="visual-command"]')
  await page.waitForSelector("[data-command-response]", { visible: true })
  await settlePage(page)
  const popup = await page.$eval(".inspector-scroll", (element) => {
    const rect = element.getBoundingClientRect()
    const maximumScroll = element.scrollHeight - element.clientHeight
    element.scrollTop = maximumScroll
    const scrollable =
      maximumScroll <= 1 ||
      (getComputedStyle(element).overflowY === "auto" &&
        element.scrollTop >= maximumScroll - 1)
    element.scrollTop = 0
    return {
      top: rect.top,
      left: rect.left,
      right: rect.right,
      bottom: rect.bottom,
      viewportWidth: innerWidth,
      viewportHeight: innerHeight,
      scrollable,
    }
  })
  assert.ok(
    popup.scrollable &&
      popup.top >= 46 &&
      popup.left >= 0 &&
      popup.right <= popup.viewportWidth &&
      popup.bottom <= popup.viewportHeight,
    `The ${scene} inspector must remain within the viewport and allow scrolling: ${JSON.stringify(popup)}`
  )
}

async function setPanel(page, name, open) {
  const selector =
    name === "tests"
      ? 'button[aria-controls="studio-navigation"]'
      : `button[aria-controls="studio-${name}-panel"]`
  const current = await page.$eval(
    selector,
    (button) => button.getAttribute("aria-expanded") === "true"
  )
  if (current !== open) await page.click(selector)
  await page.waitForFunction(
    (selector, open) =>
      document.querySelector(selector)?.getAttribute("aria-expanded") ===
      String(open),
    {},
    selector,
    open
  )
}

function operation(status, scene) {
  return {
    operationId: "visual-inspection",
    correlationId: "visual-correlation",
    operationEventsHref: `${operationPath}/events`,
    correlationEventsHref: "/correlations/visual-correlation/events",
    messageSeriesHref: `${operationPath}/message-series`,
    events: {
      kind: "observed",
      href: "/correlations/visual-correlation/events",
    },
    mode: "test",
    status,
    context: "bike-rental",
    command: "rent-bicycle",
    schemaVersion: 2,
    latestEventId: 1,
    ...(status === "completed"
      ? {
          result: {
            decision: scene === "rejected" ? "rejected" : "accepted",
            published: true,
            commandMessageId: "visual-command",
            responseMessageId: "visual-response",
          },
        }
      : {}),
  }
}

function messageSeries(scene) {
  const messages = [
    {
      kind: "command",
      messageId: "visual-command",
      correlationId: "visual-correlation",
      observationOrder: 0,
      name: "rent-bicycle",
      schemaVersion: 2,
      context: "bike-rental",
      payload:
        scene === "long-payload"
          ? {
              ...payload,
              reference: "rental-reference-" + "0123456789abcdef".repeat(16),
              notes:
                "A long payload value for checking wrapping and scroll behavior. ".repeat(
                  12
                ),
              inspection: Object.fromEntries(
                Array.from({ length: 16 }, (_, index) => [
                  `checkpoint_${String(index + 1).padStart(2, "0")}`,
                  {
                    condition: "serviceable",
                    description: "Brakes, tires, lights and frame checked.",
                  },
                ])
              ),
            }
          : payload,
    },
  ]
  const append = (kind, id, parent, name) =>
    messages.push({
      kind,
      messageId: id,
      causationId: parent,
      correlationId: "visual-correlation",
      observationOrder: messages.length,
      name,
      schemaVersion: 1,
      aggregate: { type: "bike-rental/rental-fleet", id: "city-fleet" },
      payload,
    })
  if (scene === "branching") {
    for (let branch = 1; branch <= 8; branch += 1) {
      const event = `visual-event-${branch}`
      append(
        "domain-event",
        event,
        "visual-command",
        `visual-branch-${branch}-recorded`
      )
      append(
        "integration-event",
        `visual-integration-${branch}-a`,
        event,
        `visual-branch-${branch}-published`
      )
      append(
        "integration-event",
        `visual-integration-${branch}-b`,
        event,
        `visual-branch-${branch}-indexed`
      )
    }
  } else if (scene !== "rejected") {
    append("domain-event", "visual-event", "visual-command", "bicycle-rented")
    append(
      "integration-event",
      "visual-integration",
      "visual-event",
      "bicycle-rental-started"
    )
  }
  return {
    operationId: "visual-inspection",
    correlationId: "visual-correlation",
    mode: "test",
    messageSeries: {
      messages,
      commandOutcomes: [
        {
          responseMessageId: "visual-response",
          commandMessageId: "visual-command",
          correlationId: "visual-correlation",
          observationOrder: messages.length,
          outcome:
            scene === "rejected"
              ? {
                  status: "rejected",
                  value: {
                    classification: "conflict",
                    code: "BICYCLE_UNAVAILABLE",
                    message:
                      "The requested bicycle cannot currently be rented.",
                    details: {
                      bicycle_id: "bike-42",
                      condition: "maintenance-required",
                    },
                  },
                }
              : { status: "accepted", value: null },
        },
      ],
    },
    capture: {
      settled: true,
      settledFor: "500ms",
      fidelity: "exact",
      note: "Synthetic visual fixture identities; explicit links only.",
    },
  }
}
