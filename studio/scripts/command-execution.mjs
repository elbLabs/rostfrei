import assert from "node:assert/strict"

// Command-path regressions from main, exercised through the integrated UI.
// All API requests fail closed into these fixtures; no Tracer is contacted.
export async function checkCommandExecution(browser, url) {
  const context = await browser.createBrowserContext()
  const page = await context.newPage()
  const base = "/contexts/bike-rental/commands/rent-bicycle"
  const requests = [],
    unexpected = [],
    errors = [],
    captures = []
  let testCount = 0
  const version = {
    schemaVersion: 2,
    contentType: "application/json",
    fields: ["fleet_id", "bicycle_id", "request_id", "legacy_id"]
      .map((name) => ({ name, value: { kind: "opaque" } }))
      .concat([{ name: "attempt", value: { kind: "scalar", scalar: "u64" } }]),
    payloadTemplate: {
      fleet_id: null,
      bicycle_id: null,
      request_id: null,
      legacy_id: null,
      attempt: 0,
    },
    testInputsHrefTemplate: `${base}/inputs`,
    simulateHrefTemplate: `${base}/simulate`,
    testHrefTemplate: `${base}/test`,
  }
  const catalog = {
    catalogVersion: 1,
    contexts: [
      {
        id: "bike-rental",
        label: "Bike Rental",
        aggregates: [],
        commands: [
          { id: "rent-bicycle", label: "Rent bicycle", versions: [version] },
          {
            id: "preview-only",
            label: "Preview only",
            versions: [{ ...version, testHrefTemplate: undefined }],
          },
        ],
      },
    ],
  }
  const rootId = (mode) => `ui-${mode}-root`
  const operation = (mode, status) => ({
    operationId: `ui-${mode}`,
    correlationId: "ui-correlation",
    mode,
    context: "bike-rental",
    command: "rent-bicycle",
    schemaVersion: 2,
    status,
    latestEventId: 1,
    operationEventsHref: `/op/${mode}/events`,
    correlationEventsHref: "/correlations/ui/events",
    messageSeriesHref: `/capture/${mode}`,
    events: {
      kind: mode === "test" ? "observed" : "predicted",
      href: `/op/${mode}/events`,
    },
    ...(status === "completed"
      ? {
          result: {
            decision: "accepted",
            commandMessageId: rootId(mode),
            published: mode === "test",
          },
        }
      : {}),
  })
  const series = (mode) => ({
    operationId: `ui-${mode}`,
    correlationId: "ui-correlation",
    mode,
    capture: {
      settled: true,
      settledFor: "500ms",
      fidelity: mode === "test" ? "exact" : "grouped",
    },
    messageSeries: {
      messages: [
        {
          kind: "command",
          messageId: rootId(mode),
          name: "rent-bicycle",
          context: "bike-rental",
          schemaVersion: 2,
          correlationId: "ui-correlation",
          observationOrder: 0,
          payload: { bicycle_id: "bike-42" },
        },
        {
          kind: "domain-event",
          messageId: `${mode}-event`,
          causationId: rootId(mode),
          name: "bicycle-rented",
          schemaVersion: 1,
          correlationId: "ui-correlation",
          observationOrder: 1,
          payload: { bicycle_id: "bike-42" },
        },
        ...(mode === "simulate"
          ? [
              {
                kind: "command",
                messageId: "downstream-command",
                causationId: `${mode}-event`,
                name: "audit-rental",
                context: "audit",
                schemaVersion: 1,
                correlationId: "ui-correlation",
                observationOrder: 2,
                payload: {},
              },
              {
                kind: "integration-event",
                messageId: "unlinked-message",
                name: "unlinked",
                schemaVersion: 1,
                correlationId: "ui-correlation",
                observationOrder: 3,
                payload: {},
              },
            ]
          : []),
      ],
      commandOutcomes: [
        {
          commandMessageId: rootId(mode),
          responseMessageId: `response-${mode}`,
          correlationId: "ui-correlation",
          observationOrder: 4,
          outcome: { status: "accepted", value: null },
        },
      ],
    },
  })
  try {
    await page.setViewport({ width: 1440, height: 900 })
    page.on("pageerror", (error) => errors.push(error.message))
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      if (!["fetch", "xhr"].includes(request.resourceType())) {
        void request.continue()
        return
      }
      const parsed = new URL(request.url())
      const pathname = parsed.pathname.replace(/^\/api/, "")
      const respond = (body, status = 200, headers = {}) =>
        request.respond({
          status,
          contentType: "application/json",
          headers,
          body: JSON.stringify(body),
        })
      if (pathname === "/catalog") {
        void respond(catalog)
        return
      }
      if (pathname === `${base}/inputs`) {
        void request.respond({
          status: 200,
          contentType: "application/json",
          body: '{"fields":[{"name":"fleet_id","label":"Fleet","options":[{"value":"city-fleet","label":"city-fleet"}]},{"name":"bicycle_id","label":"Bicycle","options":[{"value":"bike-42","label":"bike-42"}]},{"name":"legacy_id","label":"Legacy identity","options":[{"value":42,"label":"42"}]},{"name":"attempt","label":"Attempt","options":[{"value":9007199254740993,"label":"9007199254740993"}]}]}',
        })
        return
      }
      if (request.method() === "POST") {
        requests.push({
          path: pathname,
          body: request.postData(),
          key: request.headers()["idempotency-key"],
          authorized: Boolean(
            request.headers().authorization?.startsWith("Bearer ")
          ),
        })
        if (pathname === `${base}/simulate`) {
          void respond(operation("simulate", "queued"), 202, {
            location: "/op/simulate",
          })
          return
        }
        if (pathname === `${base}/test`) {
          testCount += 1
          void (testCount === 1
            ? respond(operation("test", "queued"), 202, {
                location: "/op/test",
              })
            : respond({ message: "Upstream response unavailable" }, 503))
          return
        }
        if (pathname === "/missing-location") {
          void respond(operation("test", "queued"), 202)
          return
        }
      }
      if (pathname.startsWith("/op/")) {
        void respond(operation(pathname.split("/").at(-1), "completed"))
        return
      }
      if (pathname.startsWith("/capture/")) {
        captures.push(pathname + parsed.search)
        void respond(series(pathname.split("/").at(-1)))
        return
      }
      unexpected.push(`${request.method()} ${pathname}`)
      void respond({ message: "Unexpected command test request" }, 501)
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector('[data-tracer-source="live"]')
    assert.equal(
      await page.$$eval(".test-row", (rows) => rows.length),
      0,
      "a catalog without a test repository must still support commands"
    )
    await page.click('button[aria-controls="studio-command-panel"]')
    const choice = 'select[aria-label="Command and schema"]'
    await page.select(choice, "bike-rental/preview-only@2")
    assert.equal(
      await page.$eval(
        '[data-command-mode="test"]',
        (button) => button.disabled
      ),
      true
    )
    await page.select(choice, "bike-rental/rent-bicycle@2")
    await page.waitForFunction(
      () =>
        document.querySelector('input[aria-label="Payload Fleet"]')?.value ===
          "city-fleet" &&
        !document.querySelector('input[aria-label="Payload Fleet"]').disabled
    )
    assert.equal(
      await page.$eval('[data-command-mode="test"]', (button) =>
        button.getAttribute("aria-pressed")
      ),
      "true",
      "Test preference survives Preview-only commands"
    )
    const requestId = await page.$eval(
      'input[aria-label="Payload Request id"]',
      (input) => input.value
    )
    assert.match(requestId, /^[0-9a-f-]{36}$/i)
    const expectedBody = `{"schemaVersion":2,"payload":{"fleet_id":"city-fleet","bicycle_id":"bike-42","request_id":"${requestId}","legacy_id":42,"attempt":9007199254740993}}`
    const panel = await page.$("#studio-command-panel")
    const before = await panel.boundingBox()
    const handle = await page.$("#studio-command-panel .studio-panel-header")
    const handleBounds = await handle.boundingBox()
    await page.mouse.move(handleBounds.x + 150, handleBounds.y + 20)
    await page.mouse.down()
    await page.mouse.move(handleBounds.x + 190, handleBounds.y + 60, {
      steps: 5,
    })
    await page.mouse.up()
    const after = await panel.boundingBox()
    assert.ok(
      Math.abs(after.x - before.x) > 20 && Math.abs(after.y - before.y) > 20,
      "command panel remains draggable"
    )
    await page.click('[data-command-mode="preview"]')
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForSelector(
      '[data-node-id="ui-simulate-root"][data-status="accepted"]'
    )
    assert.deepEqual(requests[0], {
      path: `${base}/simulate`,
      body: expectedBody,
      key: undefined,
      authorized: true,
    })
    assert.match(
      await page.$eval(".flow-heading", (element) => element.textContent),
      /Preview flow/
    )
    assert.equal(
      await page.$eval(
        '[data-node-id="downstream-command"]',
        (node) => node.dataset.status
      ),
      "idle"
    )
    assert.equal(
      await page.$$eval(
        '[data-target-id="unlinked-message"]',
        (edges) => edges.length
      ),
      0
    )
    assert.equal(
      await page.$$eval("[data-graph-edge]", (edges) => edges.length),
      2
    )

    const subjects = await page.evaluate(async () => {
      const { operationGraph } = await import("/src/lib/graph.ts")
      const operation = {
        status: "completed",
        result: { decision: "accepted", commandMessageId: "root" },
      }
      const command = (id, order) => ({
        kind: "command",
        messageId: id,
        name: id,
        context: "context",
        schemaVersion: 1,
        observationOrder: order,
      })
      const series = {
        messageSeries: {
          messages: [command("earlier", 0), command("root", 1)],
          commandOutcomes: [],
        },
      }
      const summarize = (nodes) =>
        nodes.map((node) => ({
          id: node.id,
          subject: node.subject,
          status: node.status,
        }))
      return {
        complete: summarize(operationGraph(operation, series)),
        missing: summarize(
          operationGraph(
            {
              ...operation,
              result: { decision: "accepted", commandMessageId: "missing" },
            },
            series
          )
        ),
      }
    })
    assert.deepEqual(subjects.complete, [
      { id: "earlier", subject: false, status: "idle" },
      { id: "root", subject: true, status: "accepted" },
    ])
    assert.ok(
      subjects.missing.every(
        (node) => node.subject === false && node.status === "idle"
      )
    )

    await page.click('[data-command-mode="test"]')
    const keySelector = 'input[aria-label="Idempotency key"]'
    const key = await page.$eval(keySelector, (input) => input.value)
    const setKey = (value) =>
      page.$eval(
        keySelector,
        (input, value) => {
          Object.getOwnPropertyDescriptor(
            HTMLInputElement.prototype,
            "value"
          ).set.call(input, value)
          input.dispatchEvent(new Event("input", { bubbles: true }))
        },
        value
      )
    await setKey("invalid key")
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForSelector(".command-form-error")
    assert.equal(testCount, 0)
    await setKey(key)
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForSelector(
      '[data-node-id="ui-test-root"][data-status="accepted"]'
    )
    assert.deepEqual(requests[1], {
      path: `${base}/test`,
      body: expectedBody,
      key,
      authorized: true,
    })
    assert.equal(
      await page.$eval(
        'button[aria-controls="studio-command-panel"]',
        (button) => button.getAttribute("aria-expanded")
      ),
      "false"
    )
    assert.deepEqual(captures, [
      "/capture/simulate?within=10s&settleFor=500ms",
      "/capture/test?within=10s&settleFor=500ms",
    ])
    await page.click('button[aria-controls="studio-command-panel"]')
    await page.waitForFunction(
      (previous) =>
        document.querySelector('input[aria-label="Idempotency key"]').value !==
          previous &&
        !document.querySelector('input[aria-label="Payload Fleet"]').disabled,
      {},
      key
    )
    const nextKey = await page.$eval(keySelector, (input) => input.value)
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForSelector('[data-graph-node][data-status="indeterminate"]')
    await page.click('button[aria-controls="studio-command-panel"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".command-form-error")
        ?.textContent.includes("may have reached the bus")
    )
    assert.equal(await page.$eval(keySelector, (input) => input.value), nextKey)
    assert.equal(requests[2].body, expectedBody)
    assert.equal(
      testCount,
      2,
      "ambiguous submissions are never retried automatically"
    )
    const validation = await page.evaluate(async () => {
      const { runTest, submitCommand } = await import("/src/lib/api.ts")
      const unsafe = []
      for (const href of [
        "https://untrusted.invalid/run",
        "//untrusted.invalid/run",
        "/runs/{id}",
        "/runs/x#fragment",
      ]) {
        try {
          await runTest(href)
        } catch (error) {
          unsafe.push(error.message)
        }
      }
      try {
        await submitCommand(
          "test",
          "/missing-location",
          2,
          "{}",
          "ui-missing-location"
        )
      } catch (error) {
        return { unsafe, errorName: error.name }
      }
    })
    assert.deepEqual(
      validation.unsafe,
      Array(4).fill("Tracer advertised an unsafe link")
    )
    assert.equal(validation.errorName, "CommandSubmissionIndeterminateError")
    assert.deepEqual(unexpected, [])
    assert.deepEqual(errors, [])
  } finally {
    await context.close()
  }
}
