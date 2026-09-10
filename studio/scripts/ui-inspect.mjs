import path from "node:path"
import { createInterface } from "node:readline"
import { parseArgs } from "node:util"

import {
  capturePage,
  closeStudioBrowser,
  desktopViewport,
  hasInspectionErrors,
  launchStudioBrowser,
  observePage,
  settlePage,
  startStudioServer,
  studioRoot,
} from "./browser.mjs"
import {
  installScenario,
  prepareScenario,
  scenarios,
} from "./visual-scenarios.mjs"

const { values } = parseArgs({
  options: {
    scene: { type: "string" },
    viewport: { type: "string" },
    out: { type: "string" },
    url: { type: "string" },
    headed: { type: "boolean" },
    list: { type: "boolean" },
    help: { type: "boolean" },
  },
})

if (values.help || values.list) {
  console.log(`Usage: pnpm inspect:ui [--scene NAME|all] [--viewport WIDTHxHEIGHT]
                       [--out DIRECTORY] [--headed] [--url STUDIO_URL]

Scenarios (default: overview):
${Object.entries(scenarios)
  .map(([name, description]) => `  ${name.padEnd(15)} ${description}`)
  .join("\n")}
  live            Inspect a running Studio with --url (fresh browser profile)

Artifacts: screenshot.png, state.json, accessibility.json, diagnostics.json.
Default output: studio/artifacts/inspect/<scene>-<width>x<height>/
--headed keeps Chrome open: Enter captures again; q or closing Chrome exits.
Named scenarios mock every API fetch. Only live uses the connected Tracer.`)
} else {
  await main()
}

async function main() {
  const scene = values.scene ?? (values.url ? "live" : "overview")
  if (scene !== "all" && scene !== "live" && !Object.hasOwn(scenarios, scene)) {
    throw new Error(
      `Unknown scene ${scene}. Use --list to see available scenes.`
    )
  }
  if (values.url && scene !== "live")
    throw new Error("--url requires --scene live")
  if (scene === "live" && !values.url)
    throw new Error("--scene live requires --url")
  if (scene === "all" && values.headed)
    throw new Error("Choose a single scene for --headed")
  if (values.url) {
    const url = new URL(values.url)
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.search ||
      url.hash
    ) {
      throw new Error(
        "--url must be an HTTP(S) Studio URL without credentials, query or fragment"
      )
    }
  }
  let viewport
  if (values.viewport) {
    const match = /^(\d+)x(\d+)$/.exec(values.viewport)
    if (
      !match ||
      Number(match[1]) < 320 ||
      Number(match[2]) < 320 ||
      Number(match[1]) > 7680 ||
      Number(match[2]) > 7680
    ) {
      throw new Error(
        "--viewport must be WIDTHxHEIGHT, each between 320 and 7680"
      )
    }
    viewport = {
      width: Number(match[1]),
      height: Number(match[2]),
      deviceScaleFactor: 1,
    }
  }
  let server
  let browser
  const cleanup = async () => {
    try {
      await closeStudioBrowser(browser)
    } finally {
      await server?.close()
    }
  }
  const interrupt = () => {
    void cleanup().finally(() => process.exit(130))
  }
  process.once("SIGINT", interrupt)
  process.once("SIGTERM", interrupt)
  try {
    let url = values.url
    let samples
    if (!url) {
      const started = await startStudioServer({
        define: {
          "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api"),
          "import.meta.env.VITE_TRACER_TOKEN":
            JSON.stringify("visual-inspection"),
        },
      })
      server = started.server
      url = started.url
      samples = await server.ssrLoadModule("/src/lib/sample-data.ts")
    }
    browser = await launchStudioBrowser({ headless: !values.headed })
    for (const name of scene === "all" ? Object.keys(scenarios) : [scene]) {
      const context = await browser.createBrowserContext()
      const page = await context.newPage()
      const dimensions =
        viewport ??
        (name === "narrow"
          ? { width: 390, height: 844, deviceScaleFactor: 1 }
          : desktopViewport)
      await page.setViewport(dimensions)
      page.setDefaultTimeout(10_000)
      const diagnostics = observePage(page)
      const directory = path.resolve(
        values.out ?? path.join(studioRoot, "artifacts/inspect"),
        `${name}-${dimensions.width}x${dimensions.height}`
      )
      const metadata = {
        scene: name,
        source: name === "live" ? "live" : "mocked",
        browser: await browser.version(),
        diagnostics,
      }
      const capture = async (extra = {}) => {
        const result = await capturePage(page, directory, {
          ...metadata,
          ...extra,
        })
        console.log(
          JSON.stringify({
            ...result,
            status: hasInspectionErrors(diagnostics) ? "errors" : "captured",
          })
        )
      }
      try {
        if (name !== "live")
          await installScenario(page, name, samples, diagnostics)
        await page.goto(url, { waitUntil: "networkidle0", timeout: 20_000 })
        if (name !== "live") await prepareScenario(page, name)
        await settlePage(page)
        if (hasInspectionErrors(diagnostics)) {
          throw new Error(
            `Browser or fixture errors in ${name}; see ${directory}/diagnostics.json`
          )
        }
        await capture()
        if (values.headed) {
          await interactiveCapture(browser, async () => {
            await settlePage(page)
            await capture()
          })
          if (hasInspectionErrors(diagnostics)) {
            throw new Error(
              `Browser errors after interaction; see ${directory}/diagnostics.json`
            )
          }
        }
      } catch (error) {
        await capture({ inspectionError: error.message }).catch(() => {})
        throw error
      } finally {
        if (browser.connected) await context.close()
      }
    }
  } finally {
    process.removeListener("SIGINT", interrupt)
    process.removeListener("SIGTERM", interrupt)
    await cleanup()
  }
}

async function interactiveCapture(browser, capture) {
  console.log(
    "Chrome is ready. Enter captures the current view again; q exits."
  )
  const input = createInterface({
    input: process.stdin,
    output: process.stdout,
  })
  const close = () => input.close()
  browser.once("disconnected", close)
  try {
    for await (const line of input) {
      if (line.trim() === "q") break
      await capture()
    }
  } finally {
    browser.off("disconnected", close)
    input.close()
  }
}
