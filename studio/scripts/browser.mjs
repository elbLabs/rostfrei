import { existsSync } from "node:fs"
import { mkdir, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

import puppeteer from "puppeteer-core"
import { createServer } from "vite"

export const studioRoot = fileURLToPath(new URL("..", import.meta.url))
export const desktopViewport = {
  width: 1440,
  height: 900,
  deviceScaleFactor: 1,
}

export async function startStudioServer({ define } = {}) {
  const server = await createServer({
    root: studioRoot,
    logLevel: "error",
    define,
    server: { host: "127.0.0.1", port: 0, open: false },
  })
  try {
    await server.listen()
    const address = server.httpServer.address()
    return { server, url: `http://127.0.0.1:${address.port}` }
  } catch (error) {
    await server.close()
    throw error
  }
}

export function launchStudioBrowser({ headless = true } = {}) {
  const candidates = [
    process.env.CHROME_BIN,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ].filter(Boolean)
  const executablePath = candidates.find((candidate) => existsSync(candidate))
  if (!executablePath) throw new Error("Set CHROME_BIN to a Chrome executable")
  return puppeteer.launch({ executablePath, headless })
}

export async function closeStudioBrowser(browser) {
  if (!browser) return
  const child = browser.process()
  await browser.close()
  // Chrome descendants can retain inherited pipes after the browser exits.
  // Release our read ends so completed captures do not keep Node alive.
  child?.stdout?.destroy()
  child?.stderr?.destroy()
}

// Record response metadata only: headers and request bodies can contain credentials.
export function observePage(page) {
  const diagnostics = {
    pageErrors: [],
    console: [],
    failedRequests: [],
    httpErrors: [],
    expectedHttpErrors: [],
    unexpectedRequests: [],
  }
  page.on("pageerror", (error) => diagnostics.pageErrors.push(error.message))
  page.on("console", (message) => {
    if (["error", "warn"].includes(message.type())) {
      diagnostics.console.push({ level: message.type(), text: message.text() })
    }
  })
  page.on("requestfailed", (request) => {
    diagnostics.failedRequests.push({
      method: request.method(),
      path: requestPath(request.url()),
      error: request.failure()?.errorText,
    })
  })
  page.on("response", (response) => {
    if (response.status() >= 400) {
      diagnostics.httpErrors.push({
        method: response.request().method(),
        path: requestPath(response.url()),
        status: response.status(),
      })
    }
  })
  return diagnostics
}

function requestPath(url) {
  return new URL(url).pathname
}

export async function settlePage(page) {
  await page.waitForSelector(".studio-shell", { timeout: 10_000 })
  await page.waitForFunction(() => document.fonts.status === "loaded")
  // Poll actual layout rather than sleeping through an assumed animation duration.
  let previous
  let stable = 0
  for (let attempt = 0; attempt < 50; attempt += 1) {
    const layout = await page.evaluate(() =>
      JSON.stringify(
        [
          ...document.querySelectorAll(
            "[data-graph-node], .studio-sidebar, [data-node-popup]"
          ),
        ].map((element) => {
          const rect = element.getBoundingClientRect()
          return [rect.x, rect.y, rect.width, rect.height].map(
            (value) => Math.round(value * 10) / 10
          )
        })
      )
    )
    stable = layout === previous ? stable + 1 : 0
    if (stable >= 3) return
    previous = layout
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  throw new Error("Studio layout did not settle within 5 seconds")
}

export async function capturePage(
  page,
  directory,
  { diagnostics, ...metadata }
) {
  await mkdir(directory, { recursive: true })
  const state = await page.evaluate(() => {
    const text = (element) => element?.textContent?.trim()
    const bounds = (element) => {
      const rect = element.getBoundingClientRect()
      return Object.fromEntries(
        ["x", "y", "width", "height"].map((key) => [
          key,
          Math.round(rect[key] * 10) / 10,
        ])
      )
    }
    const popup = document.querySelector("[data-node-popup]")
    return {
      title: document.title,
      viewport: { width: innerWidth, height: innerHeight },
      documentSize: {
        width: document.documentElement.scrollWidth,
        height: document.documentElement.scrollHeight,
      },
      heading: text(document.querySelector(".studio-topbar")),
      badges: [...document.querySelectorAll("[data-slot=badge]")].map(text),
      caption: text(document.querySelector(".graph-caption")),
      graphTransform: document.querySelector(".react-flow__viewport")?.style
        .transform,
      zoom: text(document.querySelector(".graph-zoom-value")),
      panels: [
        ...document.querySelectorAll("button[aria-controls^=studio-]"),
      ].map((button) => {
        const id = button.getAttribute("aria-controls")
        const panel = document.getElementById(id)
        return {
          id,
          open: button.getAttribute("aria-expanded") === "true",
          bounds: panel ? bounds(panel) : undefined,
        }
      }),
      nodes: [...document.querySelectorAll("[data-graph-node]")].map((node) => {
        const button = node.querySelector(".message-node")
        const rect = button.getBoundingClientRect()
        const centerX = rect.x + rect.width / 2
        const centerY = rect.y + rect.height / 2
        const top = document.elementFromPoint(centerX, centerY)
        return {
          ...node.dataset,
          label: button.getAttribute("aria-label"),
          bounds: bounds(button),
          inViewport:
            rect.right > 0 &&
            rect.left < innerWidth &&
            rect.bottom > 0 &&
            rect.top < innerHeight,
          centerUncovered: Boolean(top && button.contains(top)),
          expanded: button.getAttribute("aria-expanded") === "true",
        }
      }),
      edges: [...document.querySelectorAll("[data-graph-edge]")].map(
        (edge) => ({
          sourceId: edge.dataset.sourceId,
          targetId: edge.dataset.targetId,
          relationship: edge.dataset.edgeRelationship,
        })
      ),
      popup: popup
        ? {
            text: text(popup),
            bounds: bounds(popup),
            payloads: [...popup.querySelectorAll(".payload-list")].map(
              (list) => ({
                bounds: bounds(list),
                scrollHeight: list.scrollHeight,
                clientHeight: list.clientHeight,
              })
            ),
          }
        : null,
      alerts: [...document.querySelectorAll('[role="alert"]')].map(text),
      controls: [
        ...document.querySelectorAll("button, input, select, textarea"),
      ]
        .filter((element) => element.checkVisibility())
        .map((element) => ({
          tag: element.tagName.toLowerCase(),
          label: element.getAttribute("aria-label") ?? text(element),
          disabled: element.disabled,
        })),
    }
  })
  const client = await page.createCDPSession()
  let accessibility
  try {
    const { nodes } = await client.send("Accessibility.getFullAXTree")
    accessibility = nodes
      .filter((node) => !node.ignored)
      .map((node) => ({
        id: node.nodeId,
        role: node.role?.value,
        name: node.name?.value,
        children: node.childIds,
      }))
  } finally {
    await client.detach()
  }
  const screenshot = path.join(directory, "screenshot.png")
  await page.screenshot({ path: screenshot })
  const writeJson = (name, data) =>
    writeFile(
      path.join(directory, name),
      redact(JSON.stringify(data, null, 2)) + "\n"
    )
  await Promise.all([
    writeJson("state.json", { ...metadata, ...state }),
    writeJson("accessibility.json", accessibility),
    writeJson("diagnostics.json", diagnostics),
  ])
  return {
    directory,
    screenshot,
    nodes: state.nodes.length,
    edges: state.edges.length,
  }
}

function redact(value) {
  for (const name of [
    "ROSTFREI_API_TOKEN",
    "ROSTFREI_DISPATCH_TOKEN",
    "VITE_TRACER_TOKEN",
  ]) {
    if (process.env[name])
      value = value.replaceAll(process.env[name], "[redacted]")
  }
  return value.replace(/Bearer\s+[^\s"\\]+/gi, "Bearer [redacted]")
}

export function hasInspectionErrors(diagnostics) {
  return Boolean(
    diagnostics.pageErrors.length ||
    diagnostics.failedRequests.length ||
    diagnostics.unexpectedRequests.length ||
    diagnostics.httpErrors.some(
      (actual) =>
        !diagnostics.expectedHttpErrors.some(
          (expected) =>
            expected.method === actual.method &&
            expected.path === actual.path &&
            expected.status === actual.status
        )
    )
  )
}
