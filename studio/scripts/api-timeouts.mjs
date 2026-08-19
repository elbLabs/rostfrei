import assert from "node:assert/strict"

// Exercise the real client and fetch with held HTTP fixtures. Only the deadline
// clock is controlled, so the 10–12 second boundary is deterministic and fast.
export async function checkApiTimeouts(browser, url) {
  const context = await browser.createBrowserContext()
  const page = await context.newPage()
  const held = new Map()
  const requests = []
  const unexpected = []
  const errors = []
  let holdCatalog = false
  const operation = {
    operationId: "timeout-test",
    correlationId: "timeout-test",
    mode: "test",
    context: "timeout",
    command: "capture",
    schemaVersion: 1,
    status: "completed",
    latestEventId: 1,
    operationEventsHref: "/operations/timeout-test/events",
    correlationEventsHref: "/correlations/timeout-test/events",
    messageSeriesHref: "/operations/timeout-test/message-series",
    events: { kind: "observed", href: "/correlations/timeout-test/events" },
    result: { decision: "accepted", commandMessageId: "partial-command" },
  }
  const series = {
    operationId: operation.operationId,
    correlationId: operation.correlationId,
    mode: "test",
    capture: { settled: false, settledFor: "500ms", fidelity: "exact" },
    messageSeries: {
      messages: [
        {
          kind: "command",
          messageId: "partial-command",
          name: "capture",
          context: "timeout",
          schemaVersion: 1,
          observationOrder: 0,
          payload: {},
        },
      ],
      commandOutcomes: [],
    },
  }
  const capturePath = `/api${operation.messageSeriesHref}`
  const heldPaths = [
    "/api/catalog",
    capturePath,
    "/api/test-scenario/fixtures/cancelled",
    "/api/test-scenario/fixtures/shorter",
    "/api/timeout-run",
  ]

  try {
    page.on("pageerror", (error) => errors.push(error.message))
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      if (!["fetch", "xhr"].includes(request.resourceType())) {
        void request.continue()
        return
      }
      const parsed = new URL(request.url())
      const pathname = parsed.pathname
      requests.push(`${request.method()} ${pathname}${parsed.search}`)
      const respond = (body, status = 200, headers = {}) =>
        request.respond({
          status,
          headers,
          contentType: "application/json",
          body: JSON.stringify(body),
        })
      if (pathname === "/api/catalog" && !holdCatalog) {
        void respond({ catalogVersion: 1, contexts: [] })
      } else if (
        pathname === "/api/timeout-test" &&
        request.method() === "POST"
      ) {
        void respond(operation, 202, { location: "/operations/timeout-test" })
      } else if (heldPaths.includes(pathname)) {
        held.set(pathname, request)
      } else {
        unexpected.push(`${request.method()} ${pathname}`)
        void respond({ message: "Unexpected timeout fixture request" }, 501)
      }
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector('[data-tracer-source="live"]')
    requests.length = 0
    holdCatalog = true
    const pending = heldPaths.map((pathname) =>
      page.waitForRequest(
        (request) => new URL(request.url()).pathname === pathname
      )
    )
    await page.evaluate(async () => {
      const { getCatalog, getFixture, runTest, submitCommand } =
        await import("/src/lib/api.ts")
      let now = 0
      const deadlines = []
      // Keep native AbortSignals, their composition and fetch cancellation;
      // replace only native timeout scheduling in this isolated browser context.
      AbortSignal.timeout = (milliseconds) => {
        const controller = new AbortController()
        deadlines.push({ at: now + milliseconds, controller })
        return controller.signal
      }
      window.__apiTimeoutResults = {}
      const track = (name, promise) => {
        promise.then(
          (value) => (window.__apiTimeoutResults[name] = { value }),
          (error) =>
            (window.__apiTimeoutResults[name] = { error: error.message })
        )
      }
      const caller = new AbortController()
      window.__apiTimeoutClock = {
        cancel: () => caller.abort(),
        advance: (milliseconds) => {
          now += milliseconds
          for (const { at, controller } of deadlines) {
            if (at <= now && !controller.signal.aborted) {
              controller.abort(
                new DOMException("Deadline elapsed", "TimeoutError")
              )
            }
          }
        },
      }
      track("discovery", getCatalog())
      track("cancelled", getFixture("cancelled", caller.signal))
      track("shorter", getFixture("shorter", AbortSignal.timeout(500)))
      track("post", runTest("/timeout-run"))
      track(
        "capture",
        submitCommand("test", "/timeout-test", 1, "{}", "timeout-test-key")
      )
    })
    await Promise.all(pending)
    await page.evaluate(() => {
      window.__apiTimeoutClock.cancel()
      window.__apiTimeoutClock.advance(500)
    })
    await page.waitForFunction(
      () =>
        window.__apiTimeoutResults.cancelled &&
        window.__apiTimeoutResults.shorter
    )
    const cancellations = await page.evaluate(() => window.__apiTimeoutResults)
    assert.match(
      cancellations.cancelled.error,
      /Request cancelled or timed out/
    )
    assert.match(cancellations.shorter.error, /Request cancelled or timed out/)

    await page.evaluate(() => window.__apiTimeoutClock.advance(9_499))
    assert.equal(
      await page.evaluate(() => window.__apiTimeoutResults.discovery),
      undefined,
      "normal GET must retain the full 10-second budget"
    )
    await page.evaluate(() => window.__apiTimeoutClock.advance(1))
    await page.waitForFunction(() => window.__apiTimeoutResults.discovery)
    const atTenSeconds = await page.evaluate(() => window.__apiTimeoutResults)
    assert.match(
      atTenSeconds.discovery.error,
      /GET \/api\/catalog:.*request timed out/
    )
    assert.equal(
      atTenSeconds.capture,
      undefined,
      "capture must outlive the normal GET deadline"
    )
    assert.equal(
      atTenSeconds.post,
      undefined,
      "normal POST must retain its longer deadline"
    )

    await page.evaluate(() => window.__apiTimeoutClock.advance(500))
    await held.get(capturePath).respond({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(series),
    })
    await page.waitForFunction(() => window.__apiTimeoutResults.capture)
    const capture = await page.evaluate(
      () => window.__apiTimeoutResults.capture
    )
    assert.deepEqual(
      capture,
      { value: { operation, series } },
      "partial evidence at 10.5s must not become inspectionError"
    )

    const hungCapture = page.waitForRequest(
      (request) => new URL(request.url()).pathname === capturePath
    )
    await page.evaluate(async () => {
      const { submitCommand } = await import("/src/lib/api.ts")
      submitCommand(
        "test",
        "/timeout-test",
        1,
        "{}",
        "timeout-test-key-2"
      ).then((result) => (window.__apiTimeoutResults.hungCapture = result))
    })
    await hungCapture
    await page.evaluate(() => window.__apiTimeoutClock.advance(11_999))
    assert.equal(
      await page.evaluate(() => window.__apiTimeoutResults.hungCapture),
      undefined,
      "capture must retain the full 12-second budget"
    )
    await page.evaluate(() => window.__apiTimeoutClock.advance(1))
    await page.waitForFunction(() => window.__apiTimeoutResults.hungCapture)
    const timedOutCapture = await page.evaluate(
      () => window.__apiTimeoutResults.hungCapture
    )
    assert.deepEqual(timedOutCapture.operation, operation)
    assert.match(timedOutCapture.inspectionError, /request timed out/)

    await page.evaluate(() => window.__apiTimeoutClock.advance(37_499))
    assert.equal(
      await page.evaluate(() => window.__apiTimeoutResults.post),
      undefined
    )
    await page.evaluate(() => window.__apiTimeoutClock.advance(1))
    await page.waitForFunction(() => window.__apiTimeoutResults.post)
    const post = await page.evaluate(() => window.__apiTimeoutResults.post)
    assert.match(post.error, /POST \/api\/timeout-run:.*request timed out/)
    assert.deepEqual(
      requests.sort(),
      [
        "GET /api/catalog",
        `GET ${capturePath}?within=10s&settleFor=500ms`,
        `GET ${capturePath}?within=10s&settleFor=500ms`,
        "GET /api/test-scenario/fixtures/cancelled",
        "GET /api/test-scenario/fixtures/shorter",
        "POST /api/timeout-run",
        "POST /api/timeout-test",
        "POST /api/timeout-test",
      ].sort(),
      "timeouts must not retry requests or republish commands"
    )
    assert.deepEqual(unexpected, [])
    assert.deepEqual(errors, [])
  } finally {
    await context.close()
  }
}
