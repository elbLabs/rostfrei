import assert from "node:assert/strict"
import {
  capturePage,
  closeStudioBrowser,
  launchStudioBrowser,
  observePage,
  startStudioServer,
} from "./browser.mjs"

const control = "Bearer operator-control"
const dispatch = "Bearer operator-dispatch"
const inspection = "Bearer operator-inspection"
const testHref = "/offered/test"
const dispatchHref = "/offered/production"
const dispatchOperation = "/returned/production-operation"
const testOperation = "/returned/test-operation"

function operatorBackend() {
  const requests = []
  let dispatchPosts = 0
  let dispatchReads = 0
  const catalog = {
    catalogVersion: 1,
    contexts: [
      {
        id: "rentals",
        label: "Rentals",
        aggregates: [],
        commands: [
          {
            id: "rent",
            label: "Rent",
            versions: [
              {
                schemaVersion: 2,
                fields: [],
                payloadTemplate: { id: "" },
                testInputsHrefTemplate: "/offered/test-inputs",
                testHrefTemplate: testHref,
                dispatchHrefTemplate: dispatchHref,
              },
            ],
          },
        ],
      },
    ],
  }
  const snapshot = (mode, status, result) => ({
    operationId: `${mode}-id`,
    correlationId: `${mode}-correlation`,
    context: "rentals",
    command: "rent",
    schemaVersion: 2,
    mode,
    status,
    result,
  })
  return {
    requests,
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        if (!request.url.startsWith("/api/")) return next()
        const path = request.url.slice(4)
        const { authorization: token, "idempotency-key": key } = request.headers
        const method = request.method
        let body = ""
        if (method === "POST") for await (const chunk of request) body += chunk
        requests.push({ path, token, key, method, body })
        const json = (status, value, headers) => {
          response.writeHead(status, {
            "content-type": "application/json",
            ...headers,
          })
          response.end(JSON.stringify(value))
        }
        if (path === "/catalog" && token === control) return json(200, catalog)
        if (path === "/catalog" && token === inspection)
          return json(200, { ...catalog, contexts: [] })
        if (path === "/offered/test-inputs" && token === control) {
          return json(200, {
            fields: [
              {
                name: "id",
                label: "ID",
                options: [{ label: "Test rental", value: "test-id" }],
              },
            ],
          })
        }
        if (path === testHref && token === control && method === "POST") {
          return json(
            202,
            {
              idempotencyKey: key,
              ...snapshot("test", "completed", {
                decision: "accepted",
              }),
            },
            { location: testOperation }
          )
        }
        if (path === dispatchHref && token === dispatch && method === "POST") {
          dispatchPosts += 1
          if (dispatchPosts === 1) return json(504, {})
          return json(
            202,
            { ...snapshot("dispatch", "queued"), idempotencyKey: key },
            { location: dispatchOperation }
          )
        }
        if (
          path === dispatchOperation &&
          token === dispatch &&
          method === "GET"
        ) {
          dispatchReads += 1
          if (dispatchReads === 1) return json(403, {})
          return json(
            200,
            snapshot("dispatch", "completed", { decision: "rejected" })
          )
        }
        return json(403, {})
      })
    },
  }
}

export async function checkOperatorCommands() {
  const backend = operatorBackend()
  const { server, url } = await startStudioServer({
    configureServer: backend.configureServer,
    define: {
      "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api"),
      "import.meta.env.VITE_TRACER_TOKEN": JSON.stringify("operator-control"),
      "import.meta.env.VITE_TRACER_INSPECTION_TOKEN": JSON.stringify(
        "operator-inspection"
      ),
      "import.meta.env.VITE_STUDIO_WORKSPACES": JSON.stringify("observe"),
    },
  })
  let browser
  try {
    browser = await launchStudioBrowser()
    const page = await browser.newPage()
    const diagnostics = observePage(page)
    await page.setViewport({ width: 1440, height: 900 })
    await page.goto(url, { waitUntil: "networkidle0" })
    assert.equal(await page.$('[aria-label="Studio mode"]'), null)
    assert.equal(await page.$('[data-workspace-tab="tests"]'), null)
    assert.equal(await page.$('[data-workspace-tab="runs"]'), null)
    await page.click('[data-workspace-tab="commands"]')
    const root = "[data-operator-commands]"
    await page.waitForSelector(root, { visible: true, timeout: 5000 })
    const click = (name) =>
      page.locator(`${root} button::-p-text(${name})`).click()
    assert.ok(backend.requests.length > 0)
    assert.ok(
      backend.requests.every(
        ({ method, token }) => method === "GET" && token === inspection
      ),
      "the inspect-only profile must not discover or publish with control or dispatch credentials on entry"
    )

    await click("Enable Test control")
    const choice = 'select[aria-label="Test command schema"]'
    await page.waitForSelector(`${root} ${choice} option:nth-child(2)`)
    await page.select(choice, "0")
    await click("Show Test inputs")
    await page.waitForFunction(() =>
      document
        .querySelector("[data-operator-test-inputs]")
        ?.textContent.includes("test-id")
    )
    await page.type(
      'textarea[aria-label="Test payload JSON"]',
      '{"id":"test-id"}'
    )
    await click("Review Test publication")
    await click("Publish to isolated Test")
    await page.waitForSelector("[data-operator-status]", { timeout: 5000 })
    assert.equal(
      await page.$eval(
        "[data-operator-status]",
        (el) => el.dataset.operatorStatus
      ),
      "accepted"
    )

    await click("Enable Production dispatch")
    await page.type(
      'input[aria-label="Dispatch credential"]',
      "operator-dispatch"
    )
    await click("Use dispatch credential")
    const productionChoice = 'select[aria-label="Production command schema"]'
    await page.select(productionChoice, "0")
    const productionPayload = 'textarea[aria-label="Production payload JSON"]'
    assert.equal(await page.$eval(productionPayload, (el) => el.value), "")
    assert.equal(
      await page.$eval("[data-operator-test-inputs]", (el) =>
        el.checkVisibility()
      ),
      false
    )
    await page.type(
      productionPayload,
      '{"id":"production-id","counter":9007199254740993}'
    )
    await click("Review Production dispatch")
    const review = await page.waitForSelector(
      '[data-operator-review="dispatch"]'
    )
    assert.match(
      await review.evaluate((el) => el.textContent),
      /Production.*rentals.*rent.*v2.*production-id.*9007199254740993/s
    )
    assert.equal(
      backend.requests.filter(({ path }) => path === dispatchHref).length,
      0
    )
    if (process.env.OPERATOR_ARTIFACTS) {
      await capturePage(page, `${process.env.OPERATOR_ARTIFACTS}/light`, {
        diagnostics,
        scene: "operator-production-review",
      })
      await page.setViewport({ width: 390, height: 844 })
      await page.evaluate(() => document.documentElement.classList.add("dark"))
      await capturePage(page, `${process.env.OPERATOR_ARTIFACTS}/narrow-dark`, {
        diagnostics,
        scene: "operator-production-review",
      })
      await page.setViewport({ width: 1440, height: 900 })
    }
    await click("Confirm Production dispatch")
    await page.waitForSelector('[data-operator-status="unconfirmed"]')
    assert.equal(
      backend.requests.filter(({ path }) => path === dispatchHref).length,
      1
    )
    await click("Review exact retry")
    await click("Confirm Production dispatch")
    await page.waitForSelector('[data-operator-status="queued"]')
    await page.waitForSelector("[data-operator-inspection-error]")
    await click("Refresh operation status")
    await page.waitForSelector('[data-operator-status="rejected"]')
    const [first, retry] = backend.requests.filter(
      ({ path }) => path === dispatchHref
    )
    assert.ok(first.key)
    assert.deepEqual(first, retry)
    assert.ok(first.body.includes('"counter":9007199254740993'))
    assert.equal(
      backend.requests.filter(({ path }) => path === dispatchOperation).length,
      2
    )
    assert.ok(
      backend.requests
        .filter(({ token }) => token === dispatch)
        .every(({ path }) => [dispatchHref, dispatchOperation].includes(path))
    )
    assert.ok(
      backend.requests.every(
        ({ path, token }) =>
          ![dispatchHref, dispatchOperation].includes(path) ||
          token === dispatch
      )
    )
    assert.ok(
      backend.requests.some(
        ({ path, token, body }) =>
          path === testHref && token === control && body.includes("test-id")
      )
    )
    assert.ok(
      await page.evaluate(
        () =>
          !JSON.stringify({ ...localStorage, ...sessionStorage }).includes(
            "operator-dispatch"
          )
      )
    )
  } finally {
    if (browser) await closeStudioBrowser(browser)
    await server.close()
  }
}
