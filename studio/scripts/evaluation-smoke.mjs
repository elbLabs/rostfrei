import assert from "node:assert/strict"
import { mkdir, writeFile } from "node:fs/promises"
import path from "node:path"
import {
  capturePage,
  closeStudioBrowser,
  launchStudioBrowser,
  observePage,
  settlePage,
  startStudioServer,
  studioRoot,
} from "./browser.mjs"
import { chainView, correlationCapability } from "./correlation-fixture.mjs"
import {
  evaluationBase,
  evaluationResponse,
  evaluationSummary,
} from "./evaluation-fixture.mjs"

const LATEST = "nightly-2026-09-10"
const PREVIOUS = "nightly-2026-09-09"
const REJECTED = "nightly-2026-09-08"

function evaluationBackend() {
  const requests = []
  const unexpectedRequests = []
  let override
  return {
    requests,
    unexpectedRequests,
    reset() {
      requests.length = 0
      unexpectedRequests.length = 0
      override = undefined
    },
    // Replaces fixture answers: (queryName, payload) => { status, body } | undefined.
    respondWith(value) {
      override = value
    },
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        const [url] = request.url.split("?")
        const json = (status, value) => {
          response.writeHead(status, { "content-type": "application/json" })
          response.end(JSON.stringify(value))
        }
        if (url.startsWith(`${evaluationBase}/`)) {
          let source = ""
          for await (const chunk of request) source += chunk
          const pathname = url.slice(evaluationBase.length)
          const payload = source ? JSON.parse(source) : undefined
          const name = pathname.split("/")[4]
          requests.push({
            api: "evaluation",
            name,
            method: request.method,
            payload,
            token: request.headers.authorization,
          })
          const result =
            request.method === "POST"
              ? (override?.(name, payload) ??
                evaluationResponse(pathname, payload))
              : undefined
          if (result) return json(result.status, result.body)
          unexpectedRequests.push(requests.at(-1))
          return json(501, { message: `Unexpected evaluation fixture: ${url}` })
        }
        if (!url.startsWith("/api/")) return next()
        const pathname = url.slice(4)
        requests.push({ api: "tracer", path: pathname, method: request.method })
        if (pathname === "/catalog")
          return json(200, {
            catalogVersion: 1,
            contexts: [],
            correlations: correlationCapability,
          })
        const chain = /^\/correlations\/([^/]+)\/events$/.exec(pathname)
        const body = chain && chainView(decodeURIComponent(chain[1]))
        if (body) return json(200, body)
        unexpectedRequests.push(requests.at(-1))
        json(501, { message: `Unexpected Tracer fixture: ${url}` })
      })
    },
  }
}

export async function checkEvaluation() {
  const backend = evaluationBackend()
  const { server, url } = await startStudioServer({
    configureServer: backend.configureServer,
    define: {
      "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api"),
      "import.meta.env.VITE_TRACER_TOKEN": JSON.stringify("evaluation-control"),
      "import.meta.env.VITE_TRACER_INSPECTION_TOKEN": JSON.stringify(
        "evaluation-inspection"
      ),
      "import.meta.env.VITE_EVAL_API_URL": JSON.stringify(evaluationBase),
    },
  })
  let browser
  const results = []
  const artifactRoot = path.resolve(studioRoot, "artifacts/review/evaluation")
  try {
    await checkPresentationRules(server)
    console.log("Evaluation: presentation-rules passed")
    browser = await launchStudioBrowser()
    for (const [name, check] of [
      ["optional-mode", checkOptionalMode],
      ["run-matrix", checkRunMatrix],
      ["regression", checkRegression],
      ["sample-detail", checkSampleDetail],
      ["correlation-jump", checkCorrelationJump],
      ["correlation-deep-link", checkCorrelationDeepLink],
      ["failures", checkFailures],
      ["mobile-layout", checkMobileLayout],
    ]) {
      backend.reset()
      const context = await browser.createBrowserContext()
      const page = await context.newPage()
      page.setDefaultTimeout(10_000)
      page.setDefaultNavigationTimeout(30_000)
      const diagnostics = observePage(page)
      try {
        await page.setViewport({ width: 1440, height: 900 })
        await check(page, backend, url)
        assert.deepEqual(diagnostics.pageErrors, [])
        assert.deepEqual(backend.unexpectedRequests, [])
        results.push({ name, status: "passed" })
        console.log(`Evaluation: ${name} passed`)
      } catch (error) {
        const evidence = await capturePage(
          page,
          path.join(artifactRoot, name),
          {
            diagnostics,
            source: "mocked",
            scene: name,
            inspectionError: error.message,
          }
        )
        results.push({ name, status: "failed", error: error.message, evidence })
        console.error(`Evaluation: ${name} FAILED\n${error.message}`)
      } finally {
        await context.close()
      }
    }
    await mkdir(artifactRoot, { recursive: true })
    await writeFile(
      path.join(artifactRoot, "results.json"),
      JSON.stringify(results, null, 2) + "\n"
    )
    assert.equal(
      results.filter((result) => result.status === "failed").length,
      0,
      `Evaluation browser failures; see ${artifactRoot}/results.json`
    )
  } finally {
    await closeStudioBrowser(browser)
    await server.close()
  }
}

// Broker-free rules behind the view, loaded from the application source.
async function checkPresentationRules(server) {
  const { evaluationMatrix, groupVerdicts, scorerOutcome } =
    await server.ssrLoadModule("/src/lib/evaluation.ts")
  const { correlationHref, linkedCorrelation } = await server.ssrLoadModule(
    "/src/lib/deep-link.ts"
  )
  const { defaultBaseline } = await server.ssrLoadModule(
    "/src/lib/use-evaluation.ts"
  )

  const running = evaluationSummary(LATEST)
  running.samples = running.samples.filter(
    (sample) => !(sample.case_id === "extend-rental" && sample.epoch === 2)
  )
  running.cells = running.cells.filter(
    (cell) => cell.case_id !== "extend-rental"
  )
  const matrix = evaluationMatrix(running)
  assert.deepEqual(matrix.epochs, [1, 2])
  assert.deepEqual(matrix.scorers, ["conversation-quality", "events", "state"])
  assert.deepEqual(
    matrix.rows.map((row) => row.caseId),
    ["rent-city-bike", "report-damage", "extend-rental"],
    "cases keep the plan order"
  )
  const pending = matrix.rows[2]
  assert.equal(pending.samples[0].epoch, 1)
  assert.equal(pending.samples[1], undefined, "a missing sample stays a gap")
  assert.deepEqual(pending.cells, [undefined, undefined, undefined])

  const criteria = [
    { scorer: "quality", criterion: "tone", value: "pass" },
    { scorer: "quality", criterion: "answer", value: "fail" },
    { scorer: "events", value: "pass" },
  ]
  assert.equal(scorerOutcome(criteria, "quality"), "fail")
  assert.equal(scorerOutcome(criteria, "events"), "pass")
  assert.equal(scorerOutcome(criteria, "state"), undefined)

  const rescored = groupVerdicts([
    { scorer: "state", value: "fail", reasoning: "first" },
    { scorer: "quality", criterion: "tone", value: "pass", reasoning: "" },
    { scorer: "state", value: "pass", reasoning: "second" },
  ])
  assert.deepEqual(
    rescored.map((group) => [
      group.scorer,
      group.outcome,
      group.verdicts.map((entry) => entry.superseded),
    ]),
    [
      ["quality", "pass", [false]],
      ["state", "pass", [true, false]],
    ],
    "the latest verdict per scorer and criterion decides the outcome"
  )

  const id = "cid/with spaces&=#"
  assert.equal(linkedCorrelation(correlationHref(id)), id)
  assert.equal(linkedCorrelation(""), undefined)
  assert.equal(linkedCorrelation("#correlation="), undefined)
  assert.equal(linkedCorrelation("#correlation=%E0%A4%A"), undefined)

  const runs = [
    { run_id: "c", status: "completed" },
    { run_id: "b", status: "rejected" },
    { run_id: "a", status: "completed" },
  ]
  assert.equal(defaultBaseline(runs, "c"), "a", "rejected runs are no baseline")
  assert.equal(defaultBaseline(runs, "a"), undefined)
}

async function openEvaluation(page, url) {
  await page.goto(url, { waitUntil: "domcontentloaded" })
  await page.waitForSelector(".studio-shell")
  await page.click('[data-workspace-tab="evaluation"]')
  await page.waitForSelector('.connection-status[data-source="live"]')
  await page.waitForSelector('[data-eval-view="run"] [data-eval-cell]')
}

async function checkOptionalMode(page, backend, url) {
  await page.goto(url, { waitUntil: "domcontentloaded" })
  await page.waitForSelector(".studio-shell")
  assert.deepEqual(
    await page.$$eval('[role="tab"]', (tabs) =>
      tabs.map((tab) => tab.dataset.workspaceTab)
    ),
    ["commands", "tests", "runs", "observation", "correlations", "evaluation"],
    "a configured evaluation API adds one tab after the Tracer modes"
  )
  assert.equal(
    backend.requests.filter((request) => request.api === "evaluation").length,
    0,
    "the evaluation API is not read before its mode is opened"
  )
  await page.keyboard.down("Control")
  await page.keyboard.press("6")
  await page.keyboard.up("Control")
  await page.waitForSelector('[data-workspace="evaluation"]')
  await page.waitForSelector('[data-eval-view="run"] [data-eval-cell]')
  assert.match(await text(page, ".connection-status"), /Evaluation live/)
  assert.match(await text(page, ".rf-evidence-summary h1"), new RegExp(LATEST))
  const evaluation = backend.requests.filter(
    (request) => request.api === "evaluation"
  )
  assert.ok(evaluation.length > 0)
  assert.ok(
    evaluation.every(
      (request) => request.method === "POST" && request.token === undefined
    ),
    "evaluation queries are POSTs that never carry a Tracer token"
  )
  assert.deepEqual(
    evaluation.find((request) => request.name === "get-run-summary").payload,
    { run_id: LATEST },
    "the newest run is opened first"
  )
}

async function checkRunMatrix(page, backend, url) {
  await openEvaluation(page, url)
  assert.deepEqual(
    await page.$$eval(".evaluation-matrix thead th", (cells) =>
      cells.map((cell) => cell.textContent)
    ),
    ["Case", "Epoch 1", "Epoch 2", "conversation-quality", "events", "state"]
  )
  assert.deepEqual(
    await page.$$eval("[data-eval-case]", (rows) =>
      rows.map((row) => [
        row.dataset.evalCase,
        ...[...row.querySelectorAll("[data-eval-cell]")].map(
          (cell) => `${cell.dataset.status} ${cell.textContent}`
        ),
      ])
    ),
    [
      [
        "rent-city-bike",
        "green 2/2 green",
        "green 2/2 green",
        "green 2/2 green",
      ],
      ["report-damage", "flaky 1/2 flaky", "red 0/2 red", "green 2/2 green"],
      ["extend-rental", "red 0/2 red", "green 2/2 green", "green 2/2 green"],
    ],
    "each case shows one cell per scorer with its status in words"
  )
  assert.deepEqual(
    await page.$$eval(
      '[data-eval-sample="report-damage#2"] .evaluation-pip',
      (pips) =>
        pips.map((pip) => `${pip.dataset.scorer}:${pip.dataset.outcome}`)
    ),
    ["conversation-quality:fail", "events:fail", "state:pass"]
  )
  assert.equal(
    await page.$eval('[data-eval-sample="report-damage#2"]', (button) =>
      button.getAttribute("aria-label")
    ),
    "report-damage, epoch 2: conversation-quality fail, events fail, state pass",
    "a sample is named without relying on colour"
  )
  assert.match(
    await text(page, ".rf-evidence-summary"),
    /Completed.*6 of 6 finished.*9\.920 in, 768 out.*1,9 s.*model-under-test.*4f2c9d1.*support-assistant@v7/s
  )

  await page.click(".evaluation-refresh")
  await page.waitForFunction(
    () => document.querySelector(".evaluation")?.ariaBusy !== "true"
  )
  await waitFor(
    () =>
      backend.requests.filter((request) => request.name === "list-runs")
        .length >= 2
  )
  assert.ok(
    await page.$('[data-eval-cell="report-damage#events"]'),
    "refreshing keeps the opened run on screen"
  )

  await page.click('[aria-label="Expand sidebar"]')
  await page.waitForSelector("#studio-evaluation-panel.studio-sidebar-open")
  assert.deepEqual(
    await page.$$eval("[data-eval-run]", (rows) =>
      rows.map((row) => [
        row.dataset.evalRun,
        row.querySelector(".run-status").getAttribute("aria-label"),
        row.getAttribute("aria-current"),
      ])
    ),
    [
      [LATEST, "Completed", "page"],
      [PREVIOUS, "Completed", null],
      [REJECTED, "Rejected", null],
    ]
  )
  await page.click(`[data-eval-run="${REJECTED}"]`)
  await page.waitForSelector(".evaluation-findings")
  assert.match(
    await text(page, ".evaluation"),
    /Rejected.*0 of 4 finished.*unknown_case.*return-at-harbor.*unknown_event.*bicycle-leased/s
  )
  assert.equal(
    await page.$(".evaluation-matrix, [aria-label='Baseline run']"),
    null,
    "a rejected run has neither samples nor a regression to show"
  )
  assert.equal(
    await page.$("#studio-evaluation-panel.studio-sidebar-open"),
    null,
    "choosing a run returns to its detail"
  )
}

async function checkRegression(page, backend, url) {
  await openEvaluation(page, url)
  await page.waitForSelector(`[data-eval-regression-baseline="${PREVIOUS}"]`)
  assert.equal(
    await page.$eval('[aria-label="Baseline run"]', (select) => select.value),
    PREVIOUS,
    "the previous completed run is the default baseline"
  )
  const groups = () =>
    page.$$eval("[data-eval-regression]", (groups) =>
      groups.map((group) => [
        group.dataset.evalRegression,
        ...[...group.querySelectorAll("li")].map((item) =>
          [
            item.querySelector("strong").textContent,
            item.querySelector(".evaluation-regression-subject span")
              .textContent,
            ...[...item.querySelectorAll("[data-slot=badge]")].map(
              (badge) => badge.textContent
            ),
          ].join(" | ")
        ),
      ])
    )
  assert.deepEqual(await groups(), [
    [
      "flipped",
      "report-damage | events | 2/2 green | 0/2 red",
      "extend-rental | conversation-quality | 2/2 green | 0/2 red",
    ],
    ["newly-green", "report-damage | state | 0/2 red | 2/2 green"],
    ["flaky", "report-damage | conversation-quality | 2/2 green | 1/2 flaky"],
  ])
  assert.match(
    await text(page, "[data-eval-regression-baseline] .rf-evidence-facts"),
    /Flipped to red2.*Newly green1.*Flaky1.*Unchanged5.*Without baseline0/s
  )

  await page.select('[aria-label="Baseline run"]', "")
  await page.waitForFunction(
    () => !document.querySelector("[data-eval-regression-baseline]")
  )
  assert.match(await text(page, ".evaluation"), /Choose a baseline run/)

  backend.respondWith((name, payload) =>
    name === "get-regression" && payload.baseline_run_id === REJECTED
      ? {
          status: 404,
          body: {
            error: {
              classification: "not_found",
              code: "evaluation.baseline-not-found",
              message: "The baseline run is unknown.",
            },
          },
        }
      : undefined
  )
  await page.select('[aria-label="Baseline run"]', REJECTED)
  await page.waitForSelector('[data-eval-failure="baseline-not-found"]')
  assert.match(
    await text(page, '[data-eval-failure="baseline-not-found"]'),
    /Baseline run not found.*404.*The baseline run is unknown/s
  )
  assert.ok(
    await page.$('[data-eval-cell="report-damage#events"]'),
    "a failed comparison leaves the run matrix readable"
  )
  backend.respondWith(undefined)

  await page.select('[aria-label="Baseline run"]', PREVIOUS)
  await page.waitForSelector(`[data-eval-regression-baseline="${PREVIOUS}"]`)
  assert.deepEqual(
    backend.requests
      .filter((request) => request.name === "get-regression")
      .map((request) => [
        request.payload.run_id,
        request.payload.baseline_run_id,
      ]),
    [
      [LATEST, PREVIOUS],
      [LATEST, REJECTED],
      [LATEST, PREVIOUS],
    ],
    "only a chosen baseline is compared"
  )
}

async function checkSampleDetail(page, backend, url) {
  await openEvaluation(page, url)
  await page.click('[data-eval-sample="rent-city-bike#1"]')
  await page.waitForSelector('[data-eval-view="sample"] [data-eval-turn="2"]')
  assert.deepEqual(
    backend.requests.find((request) => request.name === "get-sample").payload,
    { run_id: LATEST, case_id: "rent-city-bike", epoch: 1 }
  )
  assert.match(
    await text(page, '[data-eval-turn="1"]'),
    /Turn 1.*model-under-test.*1\.240 in, 96 out.*1,9 s.*Customer.*rent a city bike.*Reply.*Which fleet/s
  )
  assert.deepEqual(
    await page.$$eval("[data-eval-cid]", (links) =>
      links.map((link) => [link.dataset.evalCid, link.getAttribute("href")])
    ),
    [
      ["visual-add-bike-7", "#correlation=visual-add-bike-7"],
      ["visual-rent-bike-42", "#correlation=visual-rent-bike-42"],
    ],
    "every turn links to its correlation"
  )
  assert.deepEqual(
    await page.$$eval("[data-eval-scorer]", (groups) =>
      groups.map((group) => [
        group.dataset.evalScorer,
        group.dataset.outcome,
        ...[...group.querySelectorAll("li")].map((item) =>
          [
            item.querySelector("strong").textContent,
            item.querySelector("[data-slot=badge]").textContent,
            item.dataset.superseded ?? "current",
          ].join(" | ")
        ),
      ])
    ),
    [
      [
        "conversation-quality",
        "pass",
        "answers-the-question | pass | current",
        "friendly-tone | pass | current",
      ],
      ["events", "pass", "Overall | pass | current"],
      ["state", "pass", "Overall | fail | true", "Overall | pass | current"],
    ],
    "verdicts are grouped by scorer, and a re-scored verdict stays visible"
  )
  assert.match(
    await text(page, '[data-eval-scorer="state"]'),
    /superseded.*not caught up.*matches the expected state/s
  )
  await page.click('[data-eval-turn="1"] summary')
  assert.match(await text(page, '[data-eval-turn="1"]'), /Sure\. Which fleet/)

  await page.click(".evaluation-back")
  await page.waitForSelector('[data-eval-view="run"] [data-eval-cell]')
  await page.click('[data-eval-sample="extend-rental#2"]')
  await page.waitForSelector('[data-eval-view="sample"] [data-eval-turn="1"]')
  assert.match(
    await text(page, '[data-eval-scorer="conversation-quality"]'),
    /answers-the-question.*friendly-tone.*curt and skips the acknowledgement/s,
    "a failing criterion shows its reasoning"
  )
  assert.equal(
    await page.$eval(
      '[data-eval-scorer="conversation-quality"]',
      (group) => group.dataset.outcome
    ),
    "fail"
  )
  await page.click('[data-eval-scorer="conversation-quality"] summary')
  assert.match(
    await text(page, '[data-eval-scorer="conversation-quality"]'),
    /judge-model-pinned/
  )
}

async function checkCorrelationJump(page, backend, url) {
  await openEvaluation(page, url)
  await page.click('[data-eval-sample="rent-city-bike#1"]')
  await page.waitForSelector('[data-eval-cid="visual-rent-bike-42"]')
  await page.click('[data-eval-cid="visual-rent-bike-42"]')
  await page.waitForSelector('[data-workspace="correlations"]')
  await page.waitForSelector(
    '[data-graph-edge][data-source-id="event:bicycle-rented"][data-target-id="message:bicycle-rental-started"]'
  )
  assert.match(await text(page, ".correlation-heading"), /visual-rent-bike-42/)
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    4,
    "the turn's correlation opens in the existing causal graph"
  )
  assert.equal(
    await page.evaluate(() => location.hash),
    "#correlation=visual-rent-bike-42"
  )
  assert.equal(
    await page.$("#studio-correlation-panel.studio-sidebar-open"),
    null,
    "the graph is not covered by the correlation finder"
  )
  assert.equal(
    await page.$eval('[aria-label="Correlation ID"]', (input) => input.value),
    "visual-rent-bike-42",
    "the finder shows which correlation was opened"
  )

  await page.click('[data-workspace-tab="evaluation"]')
  await page.waitForSelector('[data-eval-view="sample"] [data-eval-turn="2"]')
  assert.equal(
    await page.evaluate(() => location.hash),
    "",
    "leaving the linked correlation clears its link"
  )
  const reads = () =>
    backend.requests.filter(
      (request) => request.path === "/correlations/visual-rent-bike-42/events"
    ).length
  const before = reads()
  await page.click('[data-eval-cid="visual-rent-bike-42"]')
  await page.waitForSelector('[data-workspace="correlations"]')
  await waitFor(() => reads() === before + 1)
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-rented"]'
  )

  await page.goBack()
  await page.click('[data-workspace-tab="evaluation"]')
  await page.waitForSelector('[data-eval-cid="visual-add-bike-7"]')
  await page.click('[data-eval-cid="visual-add-bike-7"]')
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-added"]'
  )
  assert.equal(
    await page.$$eval("[data-graph-node]", (nodes) => nodes.length),
    1,
    "another turn replaces the graph with its own correlation"
  )
}

async function checkCorrelationDeepLink(page, backend, url) {
  await page.goto(`${url}/#correlation=visual-rent-bike-42`, {
    waitUntil: "domcontentloaded",
  })
  await page.waitForSelector('[data-workspace="correlations"]')
  await page.waitForSelector(
    '[data-graph-edge][data-source-id="event:invoice-opened"][data-target-id="message:invoice-issued"]'
  )
  assert.match(await text(page, ".correlation-heading"), /visual-rent-bike-42/)
  assert.equal(
    backend.requests.filter((request) => request.api === "evaluation").length,
    0
  )
  await page.evaluate(() => {
    location.hash = "#correlation=visual-transfer-bike-7"
  })
  await page.waitForSelector(
    '[data-graph-node][data-node-id="event:bicycle-transferred-in"]'
  )
}

async function checkFailures(page, backend, url) {
  backend.respondWith(() => ({
    status: 503,
    body: {
      code: "rostfrei.http.query-unavailable",
      message: "Query messaging is unavailable.",
    },
  }))
  await page.goto(url, { waitUntil: "domcontentloaded" })
  await page.waitForSelector(".studio-shell")
  await page.click('[data-workspace-tab="evaluation"]')
  await page.waitForSelector('[data-connection="evaluation-disconnected"]')
  assert.match(
    await text(page, '[data-connection="evaluation-disconnected"]'),
    /Evaluation API connection failed.*VITE_EVAL_API_URL.*VITE_EVAL_TARGET.*POST \/eval-api\/contexts\/evaluation\/queries\/list-runs\/schemas\/1: 503.*Query messaging is unavailable/s
  )
  assert.match(
    await text(page, ".connection-status"),
    /Evaluation disconnected/
  )
  assert.equal(await page.$("[data-eval-cell]"), null)

  backend.respondWith((name) =>
    name === "get-run-summary"
      ? {
          status: 503,
          body: {
            error: {
              classification: "unavailable",
              code: "evaluation.unavailable",
              message: "Evaluation results are unavailable.",
            },
          },
        }
      : undefined
  )
  await page.click(".connection-retry")
  await page.waitForSelector('[data-eval-failure="unavailable"]')
  assert.match(
    await text(page, '[data-eval-failure="unavailable"]'),
    /not an empty result.*Evaluation results are unavailable/s
  )

  backend.respondWith((name) =>
    name === "get-sample"
      ? {
          status: 404,
          body: {
            error: {
              classification: "not_found",
              code: "evaluation.sample-not-found",
              message: "The sample is unknown.",
            },
          },
        }
      : undefined
  )
  await page.click(".evaluation-refresh")
  await page.waitForSelector('[data-eval-sample="rent-city-bike#2"]')
  await page.click('[data-eval-sample="rent-city-bike#2"]')
  await page.waitForSelector('[data-eval-failure="sample-not-found"]')
  await page.click(".evaluation-back")
  await page.waitForSelector('[data-eval-view="run"] [data-eval-cell]')

  backend.respondWith((name) =>
    name === "list-runs" ? { status: 200, body: { runs: [] } } : undefined
  )
  await page.click(".evaluation-refresh")
  await page.waitForSelector(".evaluation-empty")
  assert.match(await text(page, ".evaluation-empty"), /No eval runs yet/)
}

async function checkMobileLayout(page, backend, url) {
  await page.setViewport({ width: 390, height: 844 })
  await openEvaluation(page, url)
  await settlePage(page)
  const fits = () =>
    page.evaluate(() => ({
      page: document.documentElement.scrollWidth <= innerWidth,
      content: [...document.querySelectorAll(".evaluation *")].every(
        (element) => element.getBoundingClientRect().right <= innerWidth + 1
      ),
      tabs: [...document.querySelectorAll('[role="tab"]')].every((tab) => {
        const bounds = tab.getBoundingClientRect()
        return bounds.left >= 0 && bounds.right <= innerWidth
      }),
    }))
  assert.deepEqual(
    await fits(),
    { page: true, content: true, tabs: true },
    "the run detail and all six tabs fit a 390px screen"
  )
  assert.equal(
    await page.$eval(
      ".evaluation-matrix thead",
      (head) => getComputedStyle(head).display
    ),
    "none",
    "narrow screens list each case with labelled cells instead of a wide table"
  )
  assert.equal(
    await page.$eval(
      '[data-eval-case="report-damage"] td:nth-of-type(3)',
      (cell) => getComputedStyle(cell, "::before").content
    ),
    '"conversation-quality"'
  )
  await page.click('[data-eval-sample="rent-city-bike#1"]')
  await page.waitForSelector('[data-eval-view="sample"] [data-eval-turn="2"]')
  await settlePage(page)
  assert.deepEqual(await fits(), { page: true, content: true, tabs: true })

  await page.click('[data-eval-cid="visual-rent-bike-42"]')
  await page.waitForSelector('[data-workspace="correlations"]')
  await page.waitForSelector('[data-message-id="event:bicycle-rented"]')
}

async function waitFor(condition) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (condition()) return
    await new Promise((resolve) => setTimeout(resolve, 50))
  }
  throw new Error(`Condition not met within 5 seconds: ${condition}`)
}

async function text(page, selector) {
  return page.$eval(selector, (element) => element.textContent)
}
