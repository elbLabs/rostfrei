import assert from "node:assert/strict"
import {
  historicalCapability,
  historicalMessages,
  historicalPage,
} from "./historical-fixture.mjs"
import { settlePage, startStudioServer } from "./browser.mjs"

export async function checkHistorical(browser, url) {
  const page = await browser.newPage()
  const errors = []
  const requests = []
  page.on("pageerror", (error) => errors.push(error.message))
  try {
    await page.setViewport({ width: 1440, height: 900 })
    await page.evaluateOnNewDocument(() =>
      localStorage.setItem("rostfrei-tracer-studio-layout-v1", "workbench")
    )
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      const address = new URL(request.url())
      if (!address.pathname.startsWith("/api/")) return void request.continue()
      requests.push({
        method: request.method(),
        path: address.pathname,
        query: address.search,
      })
      let body
      let status = 200
      if (address.pathname === "/api/catalog")
        body = {
          catalogVersion: 1,
          contexts: [],
          historical: [historicalCapability],
        }
      else if (address.pathname === "/api/historical/test/messages") {
        if (address.searchParams.get("cursor") === "older")
          body = historicalPage([
            {
              ...historicalMessages[0],
              id: "domain.2",
              messageId: "returned",
              name: "bicycle-returned",
              streamVersion: "9007199254740994",
            },
          ])
        else if (address.searchParams.get("aggregateType"))
          body = historicalPage(
            [historicalMessages[0]],
            "/historical/test/messages?cursor=older"
          )
        else body = historicalPage()
      } else {
        status = 501
        body = { message: "Unexpected history request" }
      }
      void request.respond({
        status,
        contentType: "application/json",
        body: JSON.stringify(body),
      })
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    assert.ok(
      await page.$('[data-studio-mode="historical"]'),
      "Historical must be a third Studio mode"
    )
    await page.click('[data-studio-mode="historical"]')
    await page.waitForSelector('[aria-label="Historical aggregate type"]')
    await page.select(
      '[aria-label="Historical aggregate type"]',
      "bike-rental/rental-fleet"
    )
    await page.type('[aria-label="Historical aggregate ID"]', "city-fleet")
    await page.click('[aria-label="Search historical data"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("9007199254740993")
    )
    assert.match(
      await page.$eval(".historical-heading", (e) => e.textContent),
      /Persisted.*partial/s
    )
    await page.click('[aria-label="Next historical page"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("bicycle-returned")
    )
    await page.click('[aria-label="Previous historical page"]')
    await page.waitForFunction(() =>
      document
        .querySelector(".message-inspector")
        ?.textContent.includes("bicycle-rented")
    )
    await page.click('[aria-label="View historical flow"]')
    await page.waitForFunction(
      () => document.querySelectorAll("[data-graph-node]").length === 3
    )
    await settlePage(page)
    assert.equal(
      await page.$$eval(
        '[data-graph-edge][data-edge-relationship="causation"]',
        (e) => e.length
      ),
      2
    )
    await page.click('[data-message-id="response.1"]')
    assert.match(
      await page.$eval(".message-inspector", (e) => e.textContent),
      /command-response/
    )
    assert.match(
      await page.$eval(".message-inspector", (e) => e.textContent),
      /request.*unavailable/i
    )
    await page.click('[data-studio-mode="executions"]')
    await page.click('[data-studio-mode="historical"]')
    await page.waitForSelector(".historical-heading")
    await page.setViewport({ width: 390, height: 844 })
    await page.click('[aria-label="Collapse sidebar"]')
    await settlePage(page)
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      ),
      true
    )
    assert.ok(
      requests.every((request) => request.method === "GET"),
      "inspection must be read-only"
    )
    assert.deepEqual(errors, [])
    console.log(
      "Verified historical navigation, aggregate/flow inspection, paging, precision and mobile layout"
    )
  } finally {
    await page.close()
  }
}

export async function checkHistoricalScopes(browser) {
  const { server, url } = await startStudioServer({
    define: {
      "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api"),
      "import.meta.env.VITE_TRACER_TOKEN": JSON.stringify("history-control"),
      "import.meta.env.VITE_TRACER_INSPECTION_TOKEN":
        JSON.stringify("history-inspection"),
    },
  })
  const page = await browser.newPage()
  const failures = []
  let inspectionOnly = false
  let stale = false
  let historyUnavailable = false
  let delayed
  let delayProduction = true
  const production = {
    ...historicalCapability,
    scope: "production",
    messagesHref: "/historical/production/messages",
  }
  page.on("pageerror", (error) => failures.push(error.message))
  try {
    await page.setViewport({ width: 1440, height: 900 })
    await page.evaluateOnNewDocument(() =>
      localStorage.setItem("rostfrei-tracer-studio-layout-v1", "workbench")
    )
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      const address = new URL(request.url())
      if (!address.pathname.startsWith("/api/")) return void request.continue()
      const inspection =
        request.headers().authorization === "Bearer history-inspection"
      let body
      let status = 200
      if (address.pathname === "/api/catalog") {
        if (inspectionOnly && !inspection) {
          status = 401
          body = { message: "Control unavailable" }
        } else
          body = {
            catalogVersion: 1,
            contexts: [],
            historical: [inspection ? production : historicalCapability],
          }
      } else if (address.pathname === "/api/historical/production/messages") {
        if (!inspection)
          failures.push("Production history used the wrong credential")
        body = {
          ...historicalPage([
            {
              ...historicalMessages[0],
              id: "production-record",
              name: "production-event",
            },
          ]),
          scope: "production",
        }
        if (delayProduction) {
          delayed = () =>
            request
              .respond({
                status,
                contentType: "application/json",
                body: JSON.stringify(body),
              })
              .catch(() => {})
          return
        }
      } else if (address.pathname === "/api/historical/test/messages") {
        if (inspection)
          failures.push("Test history used the production credential")
        if (historyUnavailable) {
          status = 503
          body = { message: "Historical storage unavailable" }
        } else if (address.searchParams.has("cursor") && stale) {
          status = 410
          body = { message: "Historical streams were reset" }
        } else
          body = historicalPage(
            [historicalMessages[0]],
            stale ? undefined : "/historical/test/messages?cursor=stale"
          )
      } else {
        status = 501
        body = { message: "Unexpected request" }
        failures.push(address.pathname)
      }
      void request.respond({
        status,
        contentType: "application/json",
        body: JSON.stringify(body),
      })
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.click('[data-studio-mode="historical"]')
    await page.waitForSelector(
      '[aria-label="Historical scope"] option[value="production"]'
    )
    await page.select('[aria-label="Historical scope"]', "production")
    const started = page.waitForRequest((request) =>
      request.url().includes("/historical/production/messages")
    )
    await page.click('[aria-label="Search historical data"]')
    await started
    await page.select('[aria-label="Historical scope"]', "test")
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-message-id="domain.1"]')
    await delayed()
    await settlePage(page)
    assert.equal(
      await page.$('[data-message-id="production-record"]'),
      null,
      "an obsolete production response must not replace Test history"
    )
    stale = true
    await page.click('[aria-label="Next historical page"]')
    await page.waitForSelector('.historical-notice[role="alert"]')
    assert.equal(
      await page.$('[data-message-id="domain.1"]'),
      null,
      "reset errors clear stale graphs"
    )
    await page.click('[aria-label="Refresh historical data"]')
    await page.waitForSelector('[data-message-id="domain.1"]')
    await page.click('[data-studio-mode="executions"]')
    historyUnavailable = true
    await page.click('[data-studio-mode="historical"]')
    await page.waitForSelector('.historical-notice[role="alert"]')
    assert.equal(
      await page.$('[data-message-id="domain.1"]'),
      null,
      "a failed read on reopening Historical clears its old graph"
    )
    await page.click('[data-studio-mode="executions"]')
    historyUnavailable = false
    await page.click('[data-studio-mode="historical"]')
    await page.waitForSelector('[data-message-id="domain.1"]')
    assert.equal(
      await page.$('.historical-notice[role="alert"]'),
      null,
      "a successful read on reopening Historical clears its old error"
    )
    delayProduction = false
    await page.select('[aria-label="Historical scope"]', "production")
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-message-id="production-record"]')
    inspectionOnly = true
    await page.reload({ waitUntil: "networkidle0" })
    await page.click('[data-studio-mode="historical"]')
    await page.waitForSelector(
      '[aria-label="Historical scope"] option[value="production"]'
    )
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-message-id="production-record"]')
    assert.deepEqual(failures, [])
    console.log(
      "Verified historical scope credentials, stale response cancellation, reset/reopen recovery and inspection-only access"
    )
  } finally {
    await page.close()
    await server.close()
  }
}
