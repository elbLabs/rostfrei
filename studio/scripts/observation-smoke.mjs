import assert from "node:assert/strict"
import {
  closeStudioBrowser,
  launchStudioBrowser,
  startStudioServer,
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
  try {
    browser = await launchStudioBrowser()
    const page = await browser.newPage()
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
    console.log(
      "Continuous observation: external flows, long-lived streams, shared cards/inspector, layout selection, pause/resume, filtering, reconnect, reset, scope credentials and mobile details passed"
    )
  } finally {
    backend.close()
    await closeStudioBrowser(browser)
    await server.close()
  }
}

if (process.argv[1]?.endsWith("observation-smoke.mjs"))
  await checkContinuousObservation()
