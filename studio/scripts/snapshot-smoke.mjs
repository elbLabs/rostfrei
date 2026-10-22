import assert from "node:assert/strict"
import { settlePage } from "./browser.mjs"
import {
  snapshotCapability,
  snapshotHistory,
  snapshotReconstruction,
} from "./snapshot-fixture.mjs"

export async function checkSnapshotInspection(browser, url) {
  const page = await browser.newPage()
  const errors = []
  const reads = []
  page.on("pageerror", (error) => errors.push(error.message))
  try {
    await page.setViewport({ width: 1440, height: 900 })
    await page.evaluateOnNewDocument(() =>
      localStorage.setItem("rostfrei-tracer-studio-layout-v1", "workbench")
    )
    await page.evaluateOnNewDocument(() => {
      const fetch = window.fetch.bind(window)
      window.fetch = (input, options) => {
        if (
          window.holdSnapshotRead &&
          String(input).includes("/historical/test/aggregate?")
        ) {
          window.holdSnapshotRead = false
          return new Promise((resolve) => {
            window.releaseSnapshotRead = (value) =>
              resolve(
                new Response(JSON.stringify(value), {
                  headers: { "content-type": "application/json" },
                })
              )
          })
        }
        return fetch(input, options)
      }
    })
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      const address = new URL(request.url())
      if (!address.pathname.startsWith("/api/")) return void request.continue()
      reads.push({
        path: address.pathname,
        query: address.search,
        method: request.method(),
      })
      let body
      let status = 200
      if (address.pathname === "/api/catalog")
        body = {
          catalogVersion: 1,
          contexts: [],
          historical: [snapshotCapability],
        }
      else if (address.pathname === "/api/historical/test/messages")
        body = snapshotHistory
      else if (address.pathname === "/api/historical/test/aggregate")
        body = snapshotReconstruction(
          address.searchParams.get("useSnapshots") !== "false",
          address.searchParams.get("version") || "6"
        )
      else if (address.pathname === "/api/historical/test/snapshot-fixture")
        body = {
          schemaVersion: 1,
          id: "city-fleet-snapshot",
          revision: "1",
          messages: [],
          snapshots: [],
        }
      else {
        status = 501
        body = { message: "Unexpected snapshot request" }
      }
      void request.respond({
        status,
        contentType: "application/json",
        body: JSON.stringify(body),
      })
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.click('[data-studio-mode="historical"]')
    await page.type('[aria-label="Historical aggregate ID"]', "city-fleet")
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-message-id="domain.1"]')
    assert.ok(
      await page.$('[aria-label="Use snapshots"]'),
      "Historical must offer snapshot-assisted reconstruction"
    )
    await page.waitForSelector('[data-reconstruction-status="hit"]')
    assert.ok(await page.$('[data-snapshot-version="4"]'))
    assert.match(
      await page.$eval(".reconstruction-summary", (e) => e.textContent),
      /2 events applied/
    )
    const assisted = await page.$eval(
      '[aria-label="Reconstructed aggregate state"]',
      (e) => e.textContent
    )
    await page.click('[aria-label="Use snapshots"]')
    await page.waitForSelector('[data-reconstruction-status="disabled"]')
    assert.match(
      await page.$eval(".reconstruction-summary", (e) => e.textContent),
      /6 events applied/
    )
    assert.equal(
      await page.$eval(
        '[aria-label="Reconstructed aggregate state"]',
        (e) => e.textContent
      ),
      assisted
    )
    assert.ok(
      await page.$('[data-snapshot-version="4"]'),
      "stored snapshots remain visible with acceleration off"
    )
    await page.click('[aria-label="Use snapshots"]')
    await page.waitForSelector('[data-reconstruction-status="hit"]')
    await page.click('[aria-label="Inspect snapshot version 4"]')
    await page.waitForSelector('[aria-label="Stored snapshot state"]')
    assert.match(
      await page.$eval(
        '[aria-label="Stored snapshot state"]',
        (e) => e.textContent
      ),
      /event_count.*4/s
    )
    const exported = page.waitForRequest((r) =>
      r.url().includes("/snapshot-fixture?")
    )
    await page.click('[aria-label="Download snapshot fixture"]')
    await exported
    // Use native typing so React's controlled input receives its normal change event.
    await page.click('[aria-label="Historical target version"]', {
      clickCount: 3,
    })
    await page.type('[aria-label="Historical target version"]', "3")
    await page.click('[aria-label="Reconstruct aggregate state"]')
    await page.waitForSelector('[data-reconstruction-status="miss"]')
    assert.match(
      await page.$eval(".reconstruction-summary", (e) => e.textContent),
      /3 events applied/
    )
    await page.evaluate(() => {
      window.holdSnapshotRead = true
    })
    await page.click('[aria-label="Use snapshots"]')
    await page.waitForFunction(() => Boolean(window.releaseSnapshotRead))
    await page.click('[aria-label="Use snapshots"]')
    await page.waitForSelector('[data-reconstruction-status="miss"]')
    await page.evaluate(
      (value) =>
        window.releaseSnapshotRead({ ...value, state: { stale: true } }),
      snapshotReconstruction(false, "3")
    )
    await settlePage(page)
    assert.ok(
      await page.$('[data-reconstruction-status="miss"]'),
      "late response from a previous replay mode must not replace current state"
    )
    assert.doesNotMatch(
      await page.$eval(
        '[aria-label="Reconstructed aggregate state"]',
        (e) => e.textContent
      ),
      /stale/
    )
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
      reads.some(
        (r) =>
          r.path.endsWith("/aggregate") &&
          r.query.includes("useSnapshots=false")
      )
    )
    assert.ok(reads.every((r) => r.method === "GET"))
    assert.deepEqual(errors, [])
    console.log(
      "Verified stored snapshots, equivalent replay on/off, target versions, fixture export, stale response fencing and responsive layout"
    )
  } finally {
    await page.close()
  }
}
