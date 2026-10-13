import assert from "node:assert/strict"
import path from "node:path"
import {
  capturePage,
  closeStudioBrowser,
  launchStudioBrowser,
  observePage,
  startStudioServer,
  studioRoot,
} from "./browser.mjs"
import {
  observationBackend,
  observationSnapshot,
} from "./observation-fixture.mjs"

export async function checkContinuousObservation() {
  const backend = observationBackend()
  const { server, url } = await startStudioServer({
    configureServer: backend.configureServer,
    define: {
      "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api"),
      "import.meta.env.VITE_TRACER_TOKEN": JSON.stringify(
        "observation-control"
      ),
      "import.meta.env.VITE_TRACER_INSPECTION_TOKEN": JSON.stringify(
        "observation-inspection"
      ),
    },
  })
  let browser
  let page
  let diagnostics
  try {
    browser = await launchStudioBrowser()
    page = await browser.newPage()
    diagnostics = observePage(page)
    const errors = []
    page.on("pageerror", (error) => errors.push(error.message))
    await page.setViewport({ width: 1440, height: 900 })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector(".studio-shell")
    assert.deepEqual(errors, [])
    await page.click('[data-layout="workbench"]')
    await page.click('[aria-label="Open observation panel"]')
    await page.waitForSelector('[data-observation-status="live"]')
    await page.click('[data-observed-flow="test-external-flow"]')
    await page.waitForSelector('[data-graph-node][data-node-id="received"]')
    backend.publish(2)
    await page.waitForSelector('[data-graph-node][data-node-id="published"]')
    assert.equal(
      await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
      2
    )
    assert.equal(
      await page.$$eval("[data-graph-edge]", (edges) => edges.length),
      1
    )
    await page.waitForFunction(
      () =>
        document.querySelector(".studio-main") &&
        [...document.querySelectorAll("[data-graph-node]")].every((node) => {
          const bounds = node.getBoundingClientRect()
          return (
            bounds.x >= 280 &&
            bounds.right <= innerWidth &&
            node.contains(
              document.elementFromPoint(
                bounds.x + bounds.width / 2,
                bounds.y + bounds.height / 2
              )
            )
          )
        })
    )
    // Re-selecting an unchanged flow must not clear its graph.
    await page.click('[data-observed-flow="test-external-flow"]')
    assert.equal(
      await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
      2
    )
    await page.click('[data-message-id="published"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("message-published")
    )
    const streamRequests = backend.requests.filter((request) =>
      request.path.endsWith("/events")
    ).length
    // Live streams must not inherit ordinary JSON requests' ten-second deadline.
    await new Promise((resolve) => setTimeout(resolve, 11_000))
    assert.equal(
      backend.requests.filter((request) => request.path.endsWith("/events"))
        .length,
      streamRequests
    )
    await page.click('[aria-label="Pause observation"]')
    await page.waitForSelector('[data-observation-status="paused"]')
    backend.publish(3)
    await new Promise((resolve) => setTimeout(resolve, 700))
    assert.equal(
      await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
      2
    )
    await page.click('[aria-label="Resume observation"]')
    await page.waitForSelector('[data-graph-node][data-node-id="delivered"]')
    assert.ok(
      await page.$eval(".message-inspector", (element) =>
        element.textContent.includes("message-published")
      )
    )
    await page.click('[data-layout="canvas"]')
    await page.click('[aria-label="Open message details"]')
    assert.ok(
      await page.$eval(".message-inspector", (element) =>
        element.textContent.includes("message-published")
      )
    )
    await page.click('[data-layout="workbench"]')
    await page.type('[aria-label="Filter observed flows"]', "unrelated")
    assert.equal(
      await page.$$eval("[data-observed-flow]", (flows) => flows.length),
      0
    )
    await page.$eval('[aria-label="Filter observed flows"]', (input) => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value"
      ).set
      setter.call(input, "")
      input.dispatchEvent(new Event("input", { bubbles: true }))
    })
    backend.disconnect()
    await page.waitForSelector('[data-observation-status="disconnected"]')
    backend.recover()
    await page.waitForSelector('[data-observation-status="live"]')
    const reset = observationSnapshot(backend.flows.test)
    backend.snapshot({
      ...reset,
      generation: "reset-generation",
      items: [],
      status: "resetting",
    })
    await page.waitForFunction(() =>
      document
        .querySelector(".observation-notice")
        ?.textContent.includes("evicted or reset")
    )
    await page.click('[aria-label="Collapse sidebar"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".observation-evidence")
        ?.textContent.includes("retained snapshot")
    )
    await page.click('[aria-label="Expand sidebar"]')
    await page.select('[aria-label="Observation scope"]', "production")
    await page.waitForSelector(
      '[data-observed-flow="production-external-flow"]'
    )
    await page.click('[data-observed-flow="production-external-flow"]')
    await page.waitForSelector('[data-graph-node][data-node-id="received"]')
    assert.equal(
      await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
      1
    )
    assert.ok(
      backend.requests.some(
        (request) =>
          request.path === "/catalog" &&
          request.token === "Bearer observation-inspection"
      )
    )
    assert.ok(
      backend.requests
        .filter((request) => request.path.includes("/production"))
        .every((request) => request.token === "Bearer observation-inspection")
    )
    let observationRevision = 2
    for (const invalid of [
      { application: "different-application" },
      { id: "different-flow" },
      { correlationId: "different-correlation" },
    ]) {
      backend.overrideDetail(invalid)
      backend.publish(observationRevision++, "production")
      await page.waitForFunction(() =>
        document
          .querySelector('[role="alert"]')
          ?.textContent.includes("Observed flow changed")
      )
      assert.equal(
        await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
        0
      )
      backend.overrideDetail(undefined)
      backend.publish(observationRevision++, "production")
      await page.waitForSelector('[data-graph-node][data-node-id="received"]')
    }
    assert.ok(backend.requests.every((request) => request.method === "GET"))
    await page.setViewport({ width: 390, height: 844 })
    await page.click('[aria-label="Close observation panel"]')
    await page.waitForSelector('.message-list [data-message-id="received"]')
    await page.click('.message-list [data-message-id="received"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("message-received")
    )
    await page.waitForFunction(() => {
      const dock = document
        .querySelector(".studio-topbar")
        .getBoundingClientRect()
      return dock.x >= 0 && dock.right <= innerWidth
    })
    assert.deepEqual(errors, [])
    await page.close()
    await checkObservationApiSafety(browser, url, backend)
    console.log(
      "Continuous observation: external flows, long-lived streams, shared cards/inspector, layout selection, pause/resume, filtering, reconnect, reset, scope credentials and mobile details passed"
    )
  } catch (error) {
    if (page && !page.isClosed()) {
      console.log(
        await capturePage(
          page,
          path.join(studioRoot, "artifacts/review/observation-failure"),
          { diagnostics, source: "mocked" }
        )
      )
    }
    throw error
  } finally {
    backend.close()
    await closeStudioBrowser(browser)
    await server.close()
  }
}

async function checkObservationApiSafety(browser, url, backend) {
  const context = await browser.createBrowserContext()
  try {
    const page = await context.newPage()
    await page.setViewport({ width: 1440, height: 900 })
    backend.inspectionOnly(true)
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector('[data-tracer-source="disconnected"]')
    await page.click('[aria-label="Open observation panel"]')
    await page.waitForSelector(
      '[data-observed-flow="production-external-flow"]'
    )
    assert.deepEqual(
      await page.$$eval('[aria-label="Observation scope"] option', (options) =>
        options.map((option) => option.value)
      ),
      ["production"]
    )
    await page.click('[data-observed-flow="production-external-flow"]')
    await page.waitForSelector('[data-graph-node][data-node-id="received"]')
    await page.click('[aria-label="Return to executions"]')
    backend.inspectionOnly(false)
    const results = await page.evaluate(async () => {
      const { followObservation } = await import("/src/lib/observation-api.ts")
      const capability = {
        application: "visual-fixture",
        scope: "test",
        eventsHref: "/observation/test/events",
        listHref: "/observation/test",
      }
      const nativeFetch = window.fetch
      const requests = []
      window.fetch = (...args) => {
        requests.push(String(args[0]))
        return nativeFetch(...args)
      }
      const unsafeErrors = []
      for (const eventsHref of [
        "https://untrusted.invalid/events",
        "//untrusted.invalid/events",
        "/\\untrusted.invalid/events",
        "/events#fragment",
        "/events\n",
        "/events/{scope}",
      ]) {
        try {
          await followObservation(
            { ...capability, eventsHref },
            new AbortController().signal,
            () => {}
          )
        } catch (error) {
          unsafeErrors.push(error.message)
        }
      }
      const frameErrors = []
      let delivered = 0
      let cancelled = 0
      for (const data of [
        JSON.stringify({ application: "other-app", scope: "test", items: [] }),
        JSON.stringify({
          application: "visual-fixture",
          scope: "production",
          items: [],
        }),
        "not-json",
        "a".repeat(1_048_577),
      ]) {
        window.fetch = async () =>
          new Response(
            new ReadableStream({
              start(controller) {
                controller.enqueue(
                  new TextEncoder().encode(
                    `event: observation\ndata: ${data}\n\n`
                  )
                )
              },
              cancel() {
                cancelled += 1
              },
            }),
            { headers: { "content-type": "text/event-stream" } }
          )
        try {
          await followObservation(
            capability,
            new AbortController().signal,
            () => {
              delivered += 1
            }
          )
        } catch (error) {
          frameErrors.push(error.message)
        }
      }
      window.fetch = async () =>
        new Response(
          new ReadableStream({
            start(controller) {
              controller.enqueue(new TextEncoder().encode("not an SSE stream"))
            },
            cancel() {
              cancelled += 1
            },
          }),
          { headers: { "content-type": "application/json" } }
        )
      let wrongContentType
      try {
        await followObservation(
          capability,
          new AbortController().signal,
          () => {
            delivered += 1
          }
        )
      } catch (error) {
        wrongContentType = error.message
      }
      window.fetch = nativeFetch
      return {
        unsafeErrors,
        requests,
        frameErrors,
        delivered,
        cancelled,
        wrongContentType,
      }
    })
    assert.equal(results.unsafeErrors.length, 6)
    assert.ok(results.unsafeErrors.every((error) => /unsafe link/.test(error)))
    assert.deepEqual(
      results.requests,
      [],
      "unsafe links must never send credentials"
    )
    assert.match(results.frameErrors[0], /scope does not match discovery/)
    assert.match(results.frameErrors[1], /scope does not match discovery/)
    assert.match(results.frameErrors[2], /JSON/)
    assert.match(results.frameErrors[3], /size limit/)
    assert.equal(results.delivered, 0)
    assert.match(
      results.wrongContentType,
      /did not return an observation stream/
    )
    assert.equal(
      results.cancelled,
      5,
      "rejected streams must release their readers and bodies"
    )

    backend.redirectEvents("/api/observation/test/redirect-target")
    const redirectError = await page.evaluate(async () => {
      const { followObservation } = await import("/src/lib/observation-api.ts")
      try {
        await followObservation(
          {
            application: "visual-fixture",
            scope: "test",
            eventsHref: "/observation/test/events",
            listHref: "/observation/test",
          },
          new AbortController().signal,
          () => {}
        )
      } catch (error) {
        return error.message
      }
    })
    backend.redirectEvents(undefined)
    assert.match(redirectError, /Could not reach the Tracer API/)
    assert.ok(
      backend.requests.every(
        (request) => !request.path.endsWith("/redirect-target")
      ),
      "authenticated SSE requests must never follow redirects"
    )

    backend.recover()
    await page.evaluate(async () => {
      const { followObservation } = await import("/src/lib/observation-api.ts")
      const nativeTimeout = window.setTimeout
      window.setTimeout = (callback, milliseconds, ...args) => {
        if (milliseconds === 35_000) window.__observationIdle = callback
        return nativeTimeout(callback, milliseconds, ...args)
      }
      window.__observationIdleResult = undefined
      window.__observationReceived = false
      void followObservation(
        {
          application: "visual-fixture",
          scope: "test",
          eventsHref: "/observation/test/events",
          listHref: "/observation/test",
        },
        new AbortController().signal,
        () => {
          window.__observationReceived = true
        }
      ).catch((error) => {
        window.__observationIdleResult = error.message
      })
    })
    await page.waitForFunction(() =>
      Boolean(window.__observationIdle && window.__observationReceived)
    )
    await page.evaluate(() => window.__observationIdle())
    await page.waitForFunction(() => Boolean(window.__observationIdleResult))
    await page.close()
    console.log(
      "Observation API safety: hostile links, scope mismatches, malformed/oversized SSE, reader cleanup and idle cancellation passed"
    )
  } finally {
    backend.inspectionOnly(false)
    backend.overrideDetail(undefined)
    backend.redirectEvents(undefined)
    await context.close()
  }
}

if (process.argv[1]?.endsWith("observation-smoke.mjs"))
  await checkContinuousObservation()
