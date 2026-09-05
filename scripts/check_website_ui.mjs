import assert from "node:assert/strict"
import { mkdir, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { fileURLToPath, pathToFileURL } from "node:url"

// Reuse the workspace's Chrome discovery and diagnostics for both frontends.
import {
  closeStudioBrowser,
  launchStudioBrowser,
  observePage,
} from "../studio/scripts/browser.mjs"

const websiteRoot = fileURLToPath(new URL("../website/", import.meta.url))
const websiteRequire = createRequire(
  new URL("../website/package.json", import.meta.url)
)
const { preview, createServer } = await import(
  pathToFileURL(websiteRequire.resolve("vite")).href
)
const base = process.env.VITE_BASE_PATH ?? "/"
assert.ok(
  base.startsWith("/") && base.endsWith("/"),
  "VITE_BASE_PATH must start and end with /"
)
const output = new URL("../artifacts/website/", import.meta.url)
await mkdir(output, { recursive: true })

const server = await preview({
  root: websiteRoot,
  configFile: false,
  base,
  logLevel: "error",
  preview: { host: "127.0.0.1", port: 0, open: false },
})
const origin = `http://127.0.0.1:${server.httpServer.address().port}`
let browser
let galleryServer
const results = []
const routes = [
  "",
  "docs",
  "docs/getting-started",
  "docs/domain-macros/command",
  "docs/messaging/subjects",
]

async function mockRepositoryMetadata(page) {
  await page.setRequestInterception(true)
  page.on("request", (request) => {
    if (request.url() === "https://api.github.com/repos/elbLabs/rostfrei") {
      void request.respond({
        status: 200,
        contentType: "application/json",
        headers: { "Access-Control-Allow-Origin": "*" },
        body: JSON.stringify({ stargazers_count: 4 }),
      })
    } else {
      void request.continue()
    }
  })
}

async function checkThemeBoot(browser) {
  for (const [stored, system, expected] of [
    ["dark", "light", "dark"],
    ["light", "dark", "light"],
    [null, "dark", "dark"],
  ]) {
    const context = await browser.createBrowserContext()
    try {
      const page = await context.newPage()
      await page.emulateMediaFeatures([
        { name: "prefers-color-scheme", value: system },
      ])
      await page.evaluateOnNewDocument((value) => {
        if (value) localStorage.setItem("rostfrei-theme", value)
      }, stored)
      await page.setRequestInterception(true)
      page.on("request", (request) => {
        if (request.resourceType() === "script") {
          void request.respond({
            status: 200,
            contentType: "text/javascript",
            body: "",
          })
        } else {
          void request.continue()
        }
      })
      await page.goto(`${origin}${base}docs/`, {
        waitUntil: "domcontentloaded",
      })
      assert.equal(
        await page.$eval("html", (element) => element.className),
        expected,
        "first-paint theme must apply before app modules load"
      )
      assert.equal(
        await page.$eval("#app", (element) => element.children.length),
        0
      )
    } finally {
      await context.close()
    }
  }
}

try {
  browser = await launchStudioBrowser()
  await checkThemeBoot(browser)
  for (const theme of ["light", "dark"]) {
    const context = await browser.createBrowserContext()
    const page = await context.newPage()
    await page.setCacheEnabled(false)
    await mockRepositoryMetadata(page)
    page.setDefaultTimeout(15_000)
    const diagnostics = observePage(page)
    await page.evaluateOnNewDocument((value) => {
      localStorage.setItem("rostfrei-theme", value)
    }, theme)
    // Explicit selection must win over the operating-system preference.
    await page.emulateMediaFeatures([
      {
        name: "prefers-color-scheme",
        value: theme === "light" ? "dark" : "light",
      },
    ])
    for (const width of [1440, 390, 320]) {
      await page.setViewport({ width, height: 900, deviceScaleFactor: 1 })
      for (const route of routes) {
        const label = `${route.replaceAll("/", "-") || "home"}-${theme}-${width}`
        try {
          const response = await page.goto(`${origin}${base}${route}`, {
            waitUntil: "networkidle0",
          })
          assert.equal(response.status(), 200, label)
          await page.waitForSelector("h1")
          await page.waitForFunction(
            (value) => document.documentElement.classList.contains(value),
            {},
            theme
          )
          // Bring deferred documentation examples into view before checking errors.
          await page.evaluate(async () => {
            for (const element of document.querySelectorAll(
              "main section, article h2"
            )) {
              element.scrollIntoView()
              await new Promise(requestAnimationFrame)
            }
            window.scrollTo(0, 0)
            await document.fonts.ready
            await Promise.all(
              [...document.images]
                .filter((image) => image.getBoundingClientRect().width > 0)
                .map((image) => image.decode())
            )
          })
          await page.waitForNetworkIdle({ idleTime: 100 })
          const state = await page.evaluate(
            (prefix) => ({
              heading: document.querySelector("h1").textContent,
              width: innerWidth,
              documentWidth: document.documentElement.scrollWidth,
              background: getComputedStyle(document.documentElement)
                .getPropertyValue("--background")
                .trim(),
              primary: getComputedStyle(document.documentElement)
                .getPropertyValue("--primary")
                .trim(),
              badLinks: [...document.querySelectorAll("a[href]")]
                .map((link) => new URL(link.href))
                .filter(
                  (url) =>
                    url.origin === location.origin &&
                    !url.pathname.startsWith(prefix)
                )
                .map((url) => url.pathname),
            }),
            base
          )
          assert.doesNotMatch(state.heading, /not found/i, label)
          assert.ok(
            state.documentWidth <= width,
            `${label}: document overflows (${state.documentWidth}px)`
          )
          assert.equal(
            state.background.toLowerCase(),
            theme === "dark" ? "#141b22" : "#edf0f2",
            label
          )
          assert.equal(
            state.primary.toLowerCase(),
            theme === "dark" ? "#f0a479" : "#9f421f",
            label
          )
          assert.deepEqual(state.badLinks, [], `${label}: base-path links`)
          assert.deepEqual(diagnostics.pageErrors, [], label)
          assert.deepEqual(diagnostics.failedRequests, [], label)
          assert.deepEqual(diagnostics.httpErrors, [], label)
          if (["", "docs"].includes(route) && width !== 320) {
            await page.screenshot({
              path: fileURLToPath(new URL(`${label}.png`, output)),
              fullPage: true,
            })
          }
          results.push({ label, ...state })
        } catch (error) {
          await page.screenshot({
            path: fileURLToPath(new URL(`${label}-failure.png`, output)),
            fullPage: true,
          })
          await writeFile(
            new URL("failure-diagnostics.json", output),
            JSON.stringify(diagnostics, null, 2)
          )
          throw error
        }
      }
      console.log(`Checked ${theme} at ${width}px: ${routes.length} routes`)
    }
    await context.close()
  }

  // The component gallery is a local library tool, on a separate Vite server.
  galleryServer = await createServer({
    configFile: fileURLToPath(
      new URL("../packages/ui/gallery/vite.config.ts", import.meta.url)
    ),
    logLevel: "error",
    server: { host: "127.0.0.1", port: 0, open: false },
  })
  await galleryServer.listen()
  const galleryOrigin = `http://127.0.0.1:${galleryServer.httpServer.address().port}`
  const page = await browser.newPage()
  const galleryDiagnostics = observePage(page)
  page.setDefaultTimeout(15_000)
  await page.emulateMediaFeatures([
    { name: "prefers-color-scheme", value: "dark" },
  ])
  await page.goto(galleryOrigin, {
    waitUntil: "networkidle0",
  })
  await page.waitForFunction(() =>
    document.documentElement.classList.contains("dark")
  )
  await page.emulateMediaFeatures([
    { name: "prefers-color-scheme", value: "light" },
  ])
  await page.waitForFunction(() =>
    document.documentElement.classList.contains("light")
  )

  for (const theme of ["light", "dark"]) {
    await page
      .locator(`::-p-aria(${theme === "light" ? "Light" : "Dark"})`)
      .click()
    await page.waitForFunction(
      (value) => document.documentElement.classList.contains(value),
      {},
      theme
    )
    for (const width of [1440, 390, 320]) {
      await page.setViewport({ width, height: 900, deviceScaleFactor: 1 })
      const state = await page.evaluate(() => ({
        documentWidth: document.documentElement.scrollWidth,
        width: innerWidth,
      }))
      assert.ok(
        state.documentWidth <= width,
        `local gallery/${theme}/${width} overflow`
      )
      assert.deepEqual(galleryDiagnostics.pageErrors, [])
      assert.deepEqual(galleryDiagnostics.failedRequests, [])
      assert.deepEqual(galleryDiagnostics.httpErrors, [])
      const label = `gallery-${theme}-${width}`
      if (width !== 320)
        await page.screenshot({
          path: fileURLToPath(new URL(`${label}.png`, output)),
          fullPage: true,
        })
      results.push({ label, ...state })
    }
  }

  // Exercise actual portalled Radix controls in the gallery: Escape must dismiss
  // the surface and return keyboard focus to its trigger in the shared shell.
  await page.setViewport({ width: 1440, height: 900, deviceScaleFactor: 1 })
  const overlays = []
  for (const trigger of await page.$$('button[aria-haspopup="dialog"]')) {
    if (await trigger.evaluate((element) => element.checkVisibility()))
      overlays.push(trigger)
  }
  assert.ok(
    overlays.length >= 2,
    "Gallery should demonstrate a dialog and popover"
  )
  for (const trigger of overlays) {
    await trigger.click()
    await page.waitForSelector('[role="dialog"]', { visible: true })
    await page.keyboard.press("Escape")
    await page.waitForSelector('[role="dialog"]', { hidden: true })
    // Radix restores focus in deferred unmount cleanup, after the surface hides.
    await page.waitForFunction(
      (element) => document.activeElement === element,
      {},
      trigger
    )
    assert.ok(
      await trigger.evaluate((element) => document.activeElement === element),
      "Overlay should restore trigger focus"
    )
  }
  const tabs = await page.$$('[role="tab"]')
  assert.ok(tabs.length >= 2, "Gallery should demonstrate interactive tabs")
  for (const tab of tabs) {
    await tab.click()
    await page.waitForFunction(
      (element) => element.getAttribute("aria-selected") === "true",
      {},
      tab
    )
  }

  await writeFile(
    new URL("results.json", output),
    JSON.stringify(results, null, 2)
  )
  console.log(
    `PASS: ${results.length} website/docs and local gallery theme/viewport checks; deployment base ${base}; live system-theme changes; overlay dismissal/focus and tabs. Screenshots: artifacts/website/`
  )
} finally {
  await closeStudioBrowser(browser)
  await galleryServer?.close()
  await new Promise((resolve, reject) =>
    server.httpServer.close((error) => (error ? reject(error) : resolve()))
  )
}
