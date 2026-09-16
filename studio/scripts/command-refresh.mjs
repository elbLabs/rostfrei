import assert from "node:assert/strict"

// Drive the real form with changing discovery results. No request may reach Tracer.
export async function checkCommandRefresh(browser, url) {
  const context = await browser.createBrowserContext()
  const page = await context.newPage()
  const base = "/contexts/refresh/commands/update"
  const large = "9007199254740993"
  const adjacent = "9007199254740992"
  let options = [large, adjacent]
  let inputRequests = 0
  let inputsUnavailable = false
  const publications = []
  const unexpected = []
  const errors = []
  const catalog = {
    catalogVersion: 1,
    contexts: [
      {
        id: "refresh",
        label: "Refresh",
        aggregates: [],
        commands: [
          {
            id: "update",
            label: "Update",
            versions: [
              {
                schemaVersion: 1,
                contentType: "application/json",
                fields: [
                  { name: "fleet_id", value: { kind: "opaque" } },
                  { name: "note", value: { kind: "scalar", scalar: "string" } },
                  { name: "attempt", value: { kind: "scalar", scalar: "u64" } },
                ],
                payloadTemplate: { fleet_id: null, note: "", attempt: 0 },
                testInputsHrefTemplate: `${base}/inputs`,
                simulateHrefTemplate: `${base}/simulate`,
                testHrefTemplate: `${base}/test`,
              },
            ],
          },
        ],
      },
    ],
  }
  const operation = {
    operationId: "refresh",
    correlationId: "refresh",
    mode: "test",
    context: "refresh",
    command: "update",
    schemaVersion: 1,
    status: "completed",
    latestEventId: 1,
    operationEventsHref: "/operations/refresh/events",
    correlationEventsHref: "/correlations/refresh/events",
    messageSeriesHref: "/operations/refresh/message-series",
    events: { kind: "observed", href: "/correlations/refresh/events" },
    result: { decision: "accepted", commandMessageId: "refresh-command" },
  }
  const series = {
    operationId: "refresh",
    correlationId: "refresh",
    mode: "test",
    capture: { settled: true, settledFor: "500ms", fidelity: "exact" },
    messageSeries: {
      messages: [
        {
          kind: "command",
          messageId: "refresh-command",
          correlationId: "refresh",
          observationOrder: 0,
          name: "update",
          context: "refresh",
          schemaVersion: 1,
          payload: {},
        },
      ],
      commandOutcomes: [
        {
          responseMessageId: "response",
          commandMessageId: "refresh-command",
          correlationId: "refresh",
          observationOrder: 1,
          outcome: { status: "accepted", value: null },
        },
      ],
    },
  }

  try {
    await page.setViewport({ width: 1440, height: 900 })
    page.on("pageerror", (error) => errors.push(error.message))
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      if (!["fetch", "xhr"].includes(request.resourceType())) {
        void request.continue()
        return
      }
      const pathname = new URL(request.url()).pathname.replace(/^\/api/, "")
      let body, raw
      let status = 200
      const headers = {}
      if (pathname === "/catalog") body = catalog
      else if (pathname === `${base}/inputs`) {
        inputRequests += 1
        if (inputsUnavailable) {
          status = 503
          body = { message: "Inputs unavailable" }
        } else {
          raw = `{"fields":[{"name":"fleet_id","label":"Fleet","options":[{"value":"fleet-A","label":"Fleet A"}]},{"name":"attempt","label":"Attempt","options":[${options.map((value) => `{"value":${value},"label":"${value}"}`).join(",")}]}]}`
        }
      } else if (pathname === `${base}/test`) {
        publications.push(request.postData())
        status = 202
        headers.location = "/operations/refresh"
        body = operation
      } else if (pathname === operation.messageSeriesHref) body = series
      else {
        unexpected.push(`${request.method()} ${pathname}`)
        status = 501
        body = { message: "Missing refresh fixture" }
      }
      void request.respond({
        status,
        headers,
        contentType: "application/json",
        body: raw ?? JSON.stringify(body),
      })
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector('[data-tracer-source="live"]')
    await page.click('button[aria-controls="studio-command-panel"]')
    await page.select(
      'select[aria-label="Command and schema"]',
      "refresh/update@1"
    )
    await page.waitForFunction(
      () =>
        document.querySelector('input[aria-label="Payload Fleet"]')?.value ===
        "fleet-A"
    )

    const edit = async (label, value) => {
      await page.$eval(
        `input[aria-label="${label}"]`,
        (input, next) => {
          Object.getOwnPropertyDescriptor(
            HTMLInputElement.prototype,
            "value"
          ).set.call(input, next)
          input.dispatchEvent(new Event("input", { bubbles: true }))
        },
        value
      )
    }
    const payload = () =>
      page.evaluate(() => ({
        fleet: document.querySelector('input[aria-label="Payload Fleet"]')
          .value,
        note: document.querySelector('input[aria-label="Payload Note"]').value,
        attempt: document.querySelector('select[aria-label="Payload Attempt"]')
          .selectedOptions[0]?.textContent,
      }))
    const key = () =>
      page.$eval('input[aria-label="Idempotency key"]', (input) => input.value)
    const refresh = async () => {
      const requestCount = inputRequests
      const previousKey = await key()
      const response = page.waitForResponse(
        (response) => new URL(response.url()).pathname === `/api${base}/inputs`
      )
      await page.click(".command-idempotency-field button")
      await response
      await page.waitForFunction(
        () =>
          !document.querySelector('input[aria-label="Payload Fleet"]').disabled
      )
      assert.equal(inputRequests, requestCount + 1)
      assert.notEqual(await key(), previousKey)
    }

    await edit("Payload Fleet", "fleet-B")
    await edit("Payload Note", "Keep this note")
    const expected = {
      fleet: "fleet-B",
      note: "Keep this note",
      attempt: large,
    }
    options = [adjacent, large]
    await refresh()
    assert.deepEqual(
      await payload(),
      expected,
      "New key must preserve manual values and remap exact selected JSON values"
    )

    inputsUnavailable = true
    await refresh()
    assert.deepEqual(
      await payload(),
      expected,
      "failed discovery must preserve the previous payload and choices"
    )
    assert.match(
      await page.$eval(
        ".command-input-error",
        (element) => element.textContent
      ),
      /Inputs unavailable/
    )

    inputsUnavailable = false
    options = [large, adjacent]
    await refresh()
    assert.deepEqual(await payload(), expected)
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForSelector(
      '[data-node-id="refresh-command"][data-status="accepted"]'
    )
    assert.equal(
      publications[0],
      `{"schemaVersion":1,"payload":{"fleet_id":"fleet-B","note":"Keep this note","attempt":${large}}}`
    )
    assert.match(
      await page.$eval(".graph-caption", (element) => element.textContent),
      /exact causality/
    )
    await page.click('button[aria-controls="studio-command-panel"]')
    await page.waitForFunction(
      () =>
        !document.querySelector('input[aria-label="Payload Fleet"]').disabled
    )
    assert.deepEqual(
      await payload(),
      expected,
      "post-publication discovery must preserve edited payloads"
    )
    await edit("Payload Note", "")
    assert.match(
      await page.$eval(".graph-caption", (element) => element.textContent),
      /exact causality/,
      "editing the next request must not change the previous capture's fidelity"
    )

    options = [adjacent]
    await refresh()
    assert.equal(
      (await payload()).note,
      "",
      "an explicitly empty value is not uninitialized"
    )
    assert.match(
      await page.$eval(
        ".command-input-error",
        (element) => element.textContent
      ),
      /no longer available/
    )
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForSelector(".command-form-error")
    assert.equal(
      publications.length,
      1,
      "an expired option must require a new explicit selection"
    )
    await page.select('select[aria-label="Payload Attempt"]', "option:0")
    series.capture = { ...series.capture, fidelity: "grouped", settled: false }
    await page.click('#studio-command-panel button[type="submit"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".graph-caption")
        ?.textContent.includes("grouped capture / partial")
    )
    assert.equal(
      publications[1],
      `{"schemaVersion":1,"payload":{"fleet_id":"fleet-B","note":"","attempt":${adjacent}}}`
    )

    const roots = await page.evaluate(
      async ({ operation, series }) => {
        const { operationGraph } = await import("/src/lib/graph.ts")
        const root = operationGraph(operation, series)[0]
        const missingParent = operationGraph(operation, {
          ...series,
          messageSeries: {
            ...series.messageSeries,
            messages: [
              { ...series.messageSeries.messages[0], causationId: "missing" },
            ],
          },
        })[0]
        return {
          root: root.edgeFidelity ?? null,
          missing: missingParent.edgeFidelity,
          parent: missingParent.parentId ?? null,
        }
      },
      { operation, series }
    )
    assert.deepEqual(
      roots,
      { root: null, missing: "grouped", parent: null },
      "a legitimate root is distinct from a missing causal parent"
    )
    assert.deepEqual(unexpected, [])
    assert.deepEqual(errors, [])
  } finally {
    await context.close()
  }
}
