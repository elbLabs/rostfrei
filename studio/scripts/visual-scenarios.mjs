import assert from "node:assert/strict"

import { settlePage } from "./browser.mjs"

export const scenarios = {
  overview: "Expected rental flow with the Tests panel open",
  command: "Populated catalog-driven Command panel",
  accepted: "Accepted Preview with command details open",
  rejected: "Business rejection with command details open",
  loading: "Pending Preview with observing indicator",
  indeterminate: "Unconfirmed Test submission with retry details",
  branching: "A 25-message synthetic branching flow after Fit graph to view",
  "long-payload": "Nested, long payload values in a message popup",
  narrow: "Accepted command details at 390 × 844",
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
              simulateHrefTemplate: `${commandPath}/simulate`,
              testHrefTemplate: `${commandPath}/test`,
            },
          ],
        },
      ],
    },
  ],
}

export async function installScenario(page, scene, samples, diagnostics) {
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
    if (method === "GET" && pathname === "/catalog") body = catalog
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
    } else if (method === "POST" && pathname === `${commandPath}/simulate`) {
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
      .querySelector(".studio-topbar")
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
  if (scene === "overview") return
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
  if (scene !== "indeterminate")
    await page.click('[data-command-mode="preview"]')
  await page.click('#studio-command-panel button[type="submit"]')

  if (scene === "loading") {
    await page.waitForSelector('[data-graph-node][data-status="running"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".studio-topbar")
        ?.textContent?.includes("observing")
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
      document.querySelectorAll("[data-graph-node]").length === count &&
      document.querySelector('[data-node-id="visual-command"]')?.dataset
        .status === (count === 1 ? "rejected" : "accepted"),
    {},
    expectedCount
  )
  await setPanel(page, "command", false)
  await settlePage(page)
  if (scene === "branching") {
    await page.click('button[aria-label="Fit graph to view"]')
    return
  }
  await page.click('[data-node-id="visual-command"] .message-node')
  await page.waitForSelector("[data-command-response]", { visible: true })
}

async function setPanel(page, name, open) {
  const selector = `button[aria-controls="studio-${name}-panel"]`
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
    events: { kind: "predicted", href: `${operationPath}/events` },
    mode: "simulate",
    status,
    context: "bike-rental",
    command: "rent-bicycle",
    schemaVersion: 2,
    latestEventId: 1,
    ...(status === "completed"
      ? {
          result: {
            decision: scene === "rejected" ? "rejected" : "accepted",
            published: false,
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
    mode: "simulate",
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
      fidelity: "grouped",
      note: "Synthetic visual fixture identities; explicit links only.",
    },
  }
}
