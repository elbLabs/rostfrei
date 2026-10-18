import assert from "node:assert/strict"
import { capturePage, observePage } from "./browser.mjs"
import {
  SAMPLE_TESTS,
  SAMPLE_DEFINITIONS,
  SAMPLE_FIXTURES,
} from "./fixtures.mjs"

// Real Studio, closed HTTP fixtures. Exercise reset-driven input changes and
// explicitly release old requests while a newer computation is still pending.
export async function checkHandlerRefresh(browser, url, scenario) {
  if (!scenario) {
    for (const name of ["completed", "late-result", "late-error"])
      await checkHandlerRefresh(browser, url, name)
    return
  }
  const context = await browser.createBrowserContext()
  const page = await context.newPage()
  const diagnostics = observePage(page)
  const unexpected = []
  const pending = []
  const submissions = []
  const base = "/contexts/refresh/commands/update"
  const test = SAMPLE_TESTS[0]
  let attempt = 1
  const respond = (request, body, status = 200) =>
    request.respond({
      status,
      contentType: "application/json",
      body: JSON.stringify(body),
    })
  const diagnostic = (value, rejected = false) => ({
    kind: "handlerSimulation",
    committed: false,
    context: "refresh",
    command: "update",
    schemaVersion: 1,
    outcome: rejected
      ? { decision: "rejected", rejection: { code: "CURRENT_REJECTION" } }
      : { decision: "accepted" },
    streams: [
      {
        aggregateType: "refresh/counter",
        aggregateId: "one",
        baseStreamVersion: value,
        readGuard: false,
      },
    ],
    proposedEvents: rejected
      ? []
      : [
          {
            aggregateType: "refresh/counter",
            aggregateId: "one",
            proposedStreamVersion: value + 1,
            eventType: "attempted",
            schemaVersion: 1,
            payload: { attempt: value },
          },
        ],
  })
  const catalog = {
    catalogVersion: 1,
    behavioralTest: { definitionsHref: "/tests" },
    contexts: [
      {
        id: "refresh",
        label: "Refresh",
        aggregates: [],
        commands: [
          {
            id: "update",
            label: "Update",
            versions: [
              {
                schemaVersion: 1,
                contentType: "application/json",
                fields: [
                  { name: "attempt", value: { kind: "scalar", scalar: "u64" } },
                ],
                payloadTemplate: { attempt: 0 },
                testInputsHrefTemplate: `${base}/inputs`,
                testHrefTemplate: `${base}/test`,
                simulateHandlerHrefTemplate: `${base}/simulate-handler`,
              },
            ],
          },
        ],
      },
    ],
  }
  try {
    await page.setViewport({ width: 1440, height: 900 })
    await page.setRequestInterception(true)
    page.on("request", (request) => {
      if (!["fetch", "xhr"].includes(request.resourceType())) {
        void request.continue()
        return
      }
      const path = new URL(request.url()).pathname.replace(/^\/api/, "")
      let body
      if (path === "/catalog") body = catalog
      else if (path === "/tests") body = { items: [test] }
      else if (path === test.definitionHref) body = SAMPLE_DEFINITIONS[test.id]
      else if (path.startsWith("/test-scenario/fixtures/"))
        body = SAMPLE_FIXTURES[path.split("/").at(-1)]
      else if (path === `${base}/inputs`)
        body = {
          fields: [
            {
              name: "attempt",
              label: "Attempt",
              options: [{ value: attempt, label: String(attempt) }],
            },
          ],
        }
      else if (path === `${base}/simulate-handler`) {
        const value = JSON.parse(request.postData()).payload.attempt
        submissions.push(value)
        if (scenario !== "completed") {
          pending.push(request)
          return
        }
        body = diagnostic(value)
      } else if (path === test.runHref) {
        attempt += 1
        body = {
          runId: `run-${attempt}`,
          testId: test.id,
          status: "failed",
          expected: SAMPLE_DEFINITIONS[test.id].definition.expected,
          observed: { messages: [], commandOutcomes: [] },
          comparison: { status: "failed", matches: [], diagnostics: [] },
          operation: {
            operationId: `op-${attempt}`,
            status: "failed",
            failure: {
              code: "fixture",
              message: "Controlled missing observation",
            },
          },
        }
      } else {
        unexpected.push(`${request.method()} ${path}`)
        void respond(request, { message: "Unexpected fixture request" }, 501)
        return
      }
      void respond(request, body)
    })
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector(".run-button:not(:disabled)")
    await page.click('[aria-controls="studio-command-panel"]')
    await page.select('[aria-label="Command and schema"]', "refresh/update@1")
    await page.waitForFunction(
      () =>
        document.querySelector('[aria-label="Payload Attempt"]')
          ?.selectedOptions[0]?.textContent === "1"
    )
    await page.click('[data-command-mode="simulate-handler"]')
    const submitted = page.waitForRequest((request) =>
      new URL(request.url()).pathname.endsWith("/simulate-handler")
    )
    await page.click(".command-submit")
    await submitted
    if (scenario === "completed") {
      await page.waitForSelector("[data-handler-simulation-result]")
      assert.match(
        await page.$eval(
          "[data-handler-simulation-result]",
          (node) => node.textContent
        ),
        /"attempt":1/
      )
    }
    await page.click('[aria-label="Close command panel"]')
    await page.click(".run-button")
    await page.waitForFunction(() =>
      document
        .querySelector('[aria-label="Payload Attempt"]')
        ?.textContent.includes("2")
    )
    await page.click('[aria-controls="studio-command-panel"]')
    assert.match(
      await page.$eval(".command-input-error", (node) => node.textContent),
      /no longer available/
    )
    if (process.env.STUDIO_HANDLER_EVIDENCE)
      await capturePage(
        page,
        `${process.env.STUDIO_HANDLER_EVIDENCE}/${scenario}`,
        { scene: scenario, source: "mocked", diagnostics }
      )
    assert.equal(
      await page.$("[data-handler-simulation-result]"),
      null,
      "Changed Test state must hide the old handler result"
    )
    assert.equal(
      await page.$eval(".command-submit", (button) => button.disabled),
      false,
      "Invalidated request must release only its own busy state"
    )
    assert.equal(
      await page.$eval('[data-command-mode="simulate-handler"]', (button) =>
        button.getAttribute("aria-pressed")
      ),
      "true"
    )
    await page.select('[aria-label="Payload Attempt"]', "option:0")
    const next = page.waitForRequest((request) =>
      new URL(request.url()).pathname.endsWith("/simulate-handler")
    )
    await page.click(".command-submit")
    await next
    if (scenario !== "completed") {
      const oldFinished = page.waitForResponse(
        (response) => response.request() === pending[0]
      )
      await respond(
        pending[0],
        scenario === "late-error"
          ? { message: "Obsolete failure" }
          : diagnostic(1),
        scenario === "late-error" ? 500 : 200
      )
      await oldFinished
      // Flush response parsing and React's queued updates before inspecting ownership.
      await page.evaluate(
        () =>
          new Promise((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(resolve))
          )
      )
      assert.equal(
        await page.$("[data-handler-simulation-result]"),
        null,
        "Late response cannot restore an obsolete proposal"
      )
      assert.equal(
        await page.$(".command-form-error"),
        null,
        "Late error cannot replace current state"
      )
      assert.equal(
        await page.$eval(".command-submit", (button) => button.disabled),
        true,
        "Old completion must not clear a newer request's busy state"
      )
      await respond(pending[1], diagnostic(2, scenario === "late-error"))
    }
    await page.waitForSelector("[data-handler-simulation-result]")
    const result = await page.$eval(
      "[data-handler-simulation-result]",
      (node) => node.textContent
    )
    assert.match(
      result,
      scenario === "late-error" ? /CURRENT_REJECTION/ : /"attempt":2/
    )
    assert.deepEqual(
      submissions,
      [1, 2],
      "Only explicit submissions; no retry or fallback"
    )
    assert.deepEqual(unexpected, [])
    assert.deepEqual(diagnostics.pageErrors, [])
    console.log(`Handler refresh passed: ${scenario}`)
  } finally {
    await Promise.allSettled(
      pending
        .filter((request) => !request.isInterceptResolutionHandled())
        .map((request) => request.abort())
    )
    await context.close()
  }
}
