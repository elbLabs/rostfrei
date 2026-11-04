import assert from "node:assert/strict"
import { pathToFileURL } from "node:url"
import {
  launchStudioBrowser,
  closeStudioBrowser,
  settlePage,
  startStudioServer,
} from "./browser.mjs"
import {
  timelineCapability,
  timelineMessages,
  timelinePage,
  timelineReconstruction,
} from "./timeline-fixture.mjs"

export async function checkHistoricalTimeline(browser, url) {
  const page = await browser.newPage()
  const errors = []
  const reads = []
  let reset = false
  let wrongGeneration = false
  page.on("pageerror", (error) => errors.push(error.message))
  try {
    await page.setViewport({ width: 1440, height: 900 })
    await page.evaluateOnNewDocument(() => {
      localStorage.setItem("rostfrei-tracer-studio-layout-v1", "workbench")
      const fetch = window.fetch.bind(window)
      window.fetch = (input, options) => {
        if (window.holdTimelineRead && String(input).includes("cursor=")) {
          window.holdTimelineRead = false
          return new Promise((resolve) => {
            window.releaseTimelineRead = (value) =>
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
      let status = 200
      let body
      if (address.pathname === "/api/catalog")
        body = {
          catalogVersion: 1,
          contexts: [],
          historical: [timelineCapability],
        }
      else if (address.pathname.endsWith("/messages")) {
        if (address.searchParams.has("cursor")) {
          if (reset) {
            status = 410
            body = { message: "Historical streams were reset" }
          } else
            body = {
              ...timelinePage(timelineMessages.slice(2)),
              ...(wrongGeneration ? { generation: "other-generation" } : {}),
            }
        } else if (address.searchParams.has("aggregateId"))
          body = timelinePage(
            timelineMessages.filter((m) => m.kind === "domain-event")
          )
        else
          body = timelinePage(
            timelineMessages.slice(0, 2),
            "/historical/test/messages?cursor=later"
          )
      } else if (address.pathname.endsWith("/aggregate"))
        body = timelineReconstruction(address.searchParams)
      else {
        status = 501
        body = { message: "Unexpected timeline request" }
      }
      void request.respond({
        status,
        contentType: "application/json",
        body: JSON.stringify(body),
      })
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.click('[data-studio-mode="historical"]')
    await page.evaluate(() =>
      [...document.querySelectorAll("button")]
        .find((e) => e.textContent === "Message flows")
        .click()
    )
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-message-id="request.1"]')
    assert.ok(
      await page.$('[aria-label="Historical timeline"]'),
      "Historical must provide a draggable timeline for recorded milestones"
    )
    await seek(page, 1)
    assert.equal(await cards(page), 1)
    assert.doesNotMatch(await card(page, "request.1"), /Accepted/)
    await page.click('[aria-label="Load more historical milestones"]')
    await page.waitForFunction(
      () =>
        document.querySelector('[aria-label="Historical timeline"]').max === "6"
    )
    assert.equal(
      await position(page),
      1,
      "loading another page preserves the playhead"
    )
    assert.equal(
      await cards(page),
      1,
      "later pages must not reveal future outcomes"
    )
    await seek(page, 2)
    assert.equal(
      await cards(page),
      2,
      "adjacent nanosecond milestones remain individually reachable"
    )
    await seek(page, 3)
    assert.match(await card(page, "request.1"), /Accepted/)
    assert.equal(
      await cards(page),
      2,
      "a retained response updates its unambiguous command"
    )
    await seek(page, 4)
    assert.equal(
      await cards(page),
      3,
      "timestamp ties retain both pieces of evidence"
    )
    await settlePage(page)
    const viewport = await page.$eval(
      ".react-flow__viewport",
      (e) => e.style.transform
    )
    const requestPosition = await page.$eval(
      '[data-id="request.1"]',
      (e) => e.style.transform
    )
    await seek(page, 1)
    assert.doesNotMatch(await card(page, "request.1"), /Accepted/)
    assert.equal(await page.$('[data-message-id="integration.1"]'), null)
    assert.equal(
      await page.$eval(".react-flow__viewport", (e) => e.style.transform),
      viewport,
      "scrubbing must not refit the viewport"
    )
    assert.equal(
      await page.$eval('[data-id="request.1"]', (e) => e.style.transform),
      requestPosition
    )
    await seek(page, 0)
    assert.equal(await cards(page), 0)
    assert.ok(
      await page.$(".react-flow"),
      "rewinding to the start keeps the canvas mounted"
    )
    await page.click('[aria-label="Play historical timeline"]')
    await page.waitForFunction(
      () =>
        Number(
          document.querySelector('[aria-label="Historical timeline"]').value
        ) > 0
    )
    await page.click('[aria-label="Pause historical timeline"]')
    const paused = await position(page)
    await new Promise((resolve) => setTimeout(resolve, 1150))
    assert.equal(await position(page), paused)
    await seek(page, 5)
    assert.match(await card(page, "response.2"), /Rejected/)
    await page.click('[aria-label="Play historical timeline"]')
    await page.waitForFunction(
      () =>
        document.querySelector('[aria-label="Historical timeline"]').value ===
        "6"
    )
    await page.waitForSelector('[aria-label="Play historical timeline"]')
    // Exercise a real mouse drag, in addition to the keyboard's precise steps.
    const slider = await page.$('[aria-label="Historical timeline"]')
    const bounds = await slider.boundingBox()
    await page.mouse.move(
      bounds.x + bounds.width - 8,
      bounds.y + bounds.height / 2
    )
    await page.mouse.down()
    await page.mouse.move(bounds.x + 8, bounds.y + bounds.height / 2, {
      steps: 8,
    })
    await page.mouse.up()
    assert.equal(await position(page), 0)

    await page.click('[aria-label="Refresh historical data"]')
    await page.waitForFunction(
      () =>
        document.querySelector('[aria-label="Historical timeline"]')?.max ===
        "2"
    )
    await page.evaluate(() => {
      window.holdTimelineRead = true
    })
    await page.click('[aria-label="Load more historical milestones"]')
    await page.waitForFunction(() => Boolean(window.releaseTimelineRead))
    await page.evaluate(() =>
      [...document.querySelectorAll("button")]
        .find((e) => e.textContent === "Aggregates")
        .click()
    )
    await page.type('[aria-label="Historical aggregate ID"]', "city-fleet")
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector(".reconstruction-summary")
    await page.evaluate(
      (value) => window.releaseTimelineRead(value),
      timelinePage(timelineMessages.slice(2))
    )
    await settlePage(page)
    assert.equal(
      await cards(page),
      2,
      "late flow pages cannot enter a different selection"
    )
    const aggregateViewport = await page.$eval(
      ".react-flow__viewport",
      (e) => e.style.transform
    )
    assert.ok(
      await page.$eval('[data-message-id="domain.1"]', (e) => {
        const bounds = e.getBoundingClientRect()
        const viewport = e.closest(".message-graph").getBoundingClientRect()
        return bounds.top >= viewport.top && bounds.bottom <= viewport.bottom
      }),
      "state loading must leave graph cards inside the viewport"
    )
    await seek(page, 1)
    await page.waitForFunction(() =>
      document
        .querySelector(".reconstruction-summary")
        ?.textContent.includes("Version 1")
    )
    await settlePage(page)
    assert.equal(
      await page.$eval(".react-flow__viewport", (e) => e.style.transform),
      aggregateViewport,
      "state reconstruction must preserve the timeline viewport"
    )
    await page.click('[aria-label="Use snapshots"]')
    await page.waitForSelector('[data-reconstruction-status="disabled"]')
    assert.match(
      await page.$eval(".reconstruction-summary", (e) => e.textContent),
      /Version 1/
    )
    await seek(page, 0)
    await page.waitForFunction(() =>
      document
        .querySelector(".reconstruction-summary")
        ?.textContent.includes("Version 0")
    )
    await seek(page, 2)
    await page.waitForFunction(() =>
      document
        .querySelector(".reconstruction-summary")
        ?.textContent.includes("Version 2")
    )
    await page.emulateMediaFeatures([
      { name: "prefers-reduced-motion", value: "reduce" },
    ])
    await seek(page, 1)
    assert.equal(
      await page.$eval(
        ".message-card",
        (e) => getComputedStyle(e).animationName
      ),
      "none"
    )
    await page.setViewport({ width: 390, height: 844 })
    await page.click('[aria-label="Collapse sidebar"]')
    await settlePage(page)
    assert.ok(await page.$('[aria-label="Historical timeline"]'))
    assert.ok(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      )
    )
    await page.setViewport({ width: 1440, height: 900 })
    await page.click('[aria-label="Expand sidebar"]')
    await page.evaluate(() =>
      [...document.querySelectorAll("button")]
        .find((e) => e.textContent === "Message flows")
        .click()
    )
    await page.click('[aria-label="Search historical data"]')
    await page.waitForSelector('[data-message-id="request.1"]')
    wrongGeneration = true
    await page.click('[aria-label="Load more historical milestones"]')
    await page.waitForSelector('.historical-notice[role="alert"]')
    assert.equal(
      await cards(page),
      0,
      "generations cannot be joined in one timeline"
    )
    wrongGeneration = false
    await page.click('[aria-label="Refresh historical data"]')
    await page.waitForSelector('[data-message-id="request.1"]')
    reset = true
    await page.click('[aria-label="Load more historical milestones"]')
    await page.waitForSelector('.historical-notice[role="alert"]')
    assert.equal(await cards(page), 0, "reset clears historical playback")
    assert.ok(reads.some((r) => r.query.includes("order=time")))
    assert.ok(reads.every((r) => r.method === "GET"))
    assert.deepEqual(errors, [])
    console.log(
      "Verified chronological accumulation, precise milestones/ties, mouse/keyboard rewind, outcome transitions, stable layout, Play/Pause, state replay, stale/reset fencing and mobile/reduced motion"
    )
  } finally {
    await page.close()
  }
}

async function seek(page, target) {
  await page.focus('[aria-label="Historical timeline"]')
  await page.keyboard.press("Home")
  for (let i = 0; i < target; i++) await page.keyboard.press("ArrowRight")
  await page.waitForFunction(
    (value) =>
      document.querySelector('[aria-label="Historical timeline"]').value ===
      String(value),
    {},
    target
  )
}
const position = (page) =>
  page.$eval('[aria-label="Historical timeline"]', (e) => Number(e.value))
const cards = (page) =>
  page.$$eval("[data-message-id]", (elements) => elements.length)
const card = (page, id) =>
  page.$eval(`[data-message-id="${id}"]`, (e) => e.textContent)

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { server, url } = await startStudioServer({
    define: { "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api") },
  })
  let browser
  try {
    browser = await launchStudioBrowser()
    await checkHistoricalTimeline(browser, url)
  } finally {
    await closeStudioBrowser(browser)
    await server.close()
  }
}
