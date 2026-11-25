// Synthetic responses of a rostfrei evaluation application's query API, in the
// shape of its run-summary, regression and sample views. Turn correlation IDs
// reuse the correlation fixture, so the jump into Correlations shows a chain.

export const evaluationBase = "/eval-api"

const scorers = ["conversation-quality", "events", "state"]
const cases = [
  {
    name: "rent-city-bike",
    description: "A customer rents an available city bike.",
    steps: [
      {
        message: "Hi, I would like to rent a city bike for today.",
        referenceReply: "Sure. Which fleet would you like to pick it up from?",
      },
      {
        message: "City fleet, bike 42 please.",
        referenceReply: "Bike 42 is yours. Enjoy the ride!",
        expect: {
          events: [{ context: "bike-rental", eventType: "bicycle-rented" }],
          state: { bicycle_id: "bike-42", rented: true },
        },
      },
    ],
    cids: ["visual-add-bike-7", "visual-rent-bike-42"],
    replies: [
      "Happy to help. Which fleet would you like to pick it up from?",
      "Done, bike 42 from the city fleet is rented to you. Enjoy the ride!",
    ],
  },
  {
    name: "report-damage",
    description: "A customer reports a damaged bicycle.",
    steps: [
      {
        message: "The front brake of bike 7 does not work.",
        referenceReply: "Thanks for telling us. Bike 7 is now out of service.",
        expect: {
          events: [{ context: "bike-rental", eventType: "damage-reported" }],
        },
      },
    ],
    cids: ["visual-transfer-bike-7"],
    replies: ["Thank you, I have taken bike 7 out of service."],
  },
  {
    name: "extend-rental",
    description:
      "A customer extends a running rental and asks about the price.",
    steps: [
      {
        message: "Can I keep bike 42 until tomorrow, and what does it cost?",
        referenceReply: "Yes. One more day costs 12 euros.",
        expect: { state: { bicycle_id: "bike-42", days: 2 } },
      },
    ],
    cids: ["visual-truncated"],
    replies: ["Yes, you can keep bike 42 until tomorrow."],
  },
]

// Outcome per case and scorer, one letter per epoch: p passes, f fails.
const runs = [
  {
    run_id: "nightly-2026-09-10",
    status: "completed",
    requested_at: "2026-09-10T02:00:00.000Z",
    commit: "4f2c9d1",
    outcomes: {
      "rent-city-bike": {
        "conversation-quality": "pp",
        events: "pp",
        state: "pp",
      },
      "report-damage": {
        "conversation-quality": "pf",
        events: "ff",
        state: "pp",
      },
      "extend-rental": {
        "conversation-quality": "ff",
        events: "pp",
        state: "pp",
      },
    },
  },
  {
    run_id: "nightly-2026-09-09",
    status: "completed",
    requested_at: "2026-09-09T02:00:00.000Z",
    commit: "a81be07",
    outcomes: {
      "rent-city-bike": {
        "conversation-quality": "pp",
        events: "pp",
        state: "pp",
      },
      "report-damage": {
        "conversation-quality": "pp",
        events: "pp",
        state: "ff",
      },
      "extend-rental": {
        "conversation-quality": "pp",
        events: "pp",
        state: "pp",
      },
    },
  },
  {
    run_id: "nightly-2026-09-08",
    status: "rejected",
    requested_at: "2026-09-08T02:00:00.000Z",
    commit: "0c5d3aa",
    caseIds: ["rent-city-bike", "return-at-harbor"],
    findings: [
      {
        kind: "unknown_case",
        case_id: "return-at-harbor",
        detail: "the suite has no case `return-at-harbor`",
      },
      {
        kind: "unknown_event",
        case_id: "rent-city-bike",
        detail: "context `bike-rental` has no event `bicycle-leased`",
      },
    ],
  },
]

const criteria = {
  "conversation-quality": ["answers-the-question", "friendly-tone"],
  events: [null],
  state: [null],
}

function plan(run) {
  return {
    case_ids: run.caseIds ?? cases.map((item) => item.name),
    epochs: 2,
    requested_model: "model-under-test",
    commit: run.commit,
    prompt_reference: "support-assistant@v7",
  }
}

function verdicts(run, item, epoch) {
  return scorers.flatMap((scorer) => {
    const passed = run.outcomes[item.name][scorer][epoch - 1] === "p"
    // A failing scorer fails on its last criterion only.
    return criteria[scorer].map((criterion, index, all) => ({
      scorer,
      criterion,
      value: passed || index < all.length - 1 ? "pass" : "fail",
    }))
  })
}

function samples(run) {
  if (!run.outcomes) return []
  return cases.flatMap((item) =>
    [1, 2].map((epoch) => ({
      case_id: item.name,
      epoch,
      status: "completed",
      turns: item.steps.length,
      input_tokens: 1240 * item.steps.length,
      output_tokens: 96 * item.steps.length,
      latency_ms: 1850 * item.steps.length,
      failure: null,
      verdicts: verdicts(run, item, epoch),
    }))
  )
}

function cells(run) {
  if (!run.outcomes) return []
  return cases.flatMap((item) =>
    scorers.map((scorer) => {
      const outcome = run.outcomes[item.name][scorer]
      const passed = [...outcome].filter((letter) => letter === "p").length
      return {
        case_id: item.name,
        scorer,
        passed,
        scored: outcome.length,
        pass_rate: passed / outcome.length,
        status:
          passed === outcome.length ? "green" : passed === 0 ? "red" : "flaky",
      }
    })
  )
}

function overview(run) {
  const started = samples(run).length
  return {
    run_id: run.run_id,
    status: run.status,
    requested_at: run.requested_at,
    plan: plan(run),
    sample_counts: {
      planned: plan(run).case_ids.length * 2,
      started,
      finished: started,
      failed: 0,
    },
  }
}

export function evaluationRuns() {
  return { runs: runs.map(overview) }
}

export function evaluationSummary(runId) {
  const run = runs.find((item) => item.run_id === runId)
  if (!run) return undefined
  const all = samples(run)
  const sum = (field) => all.reduce((total, item) => total + item[field], 0)
  const turns = sum("turns")
  return {
    ...overview(run),
    findings: run.findings ?? [],
    totals: {
      turns,
      input_tokens: sum("input_tokens"),
      output_tokens: sum("output_tokens"),
      latency_ms: sum("latency_ms"),
      mean_turn_latency_ms: turns ? Math.floor(sum("latency_ms") / turns) : 0,
    },
    cells: cells(run),
    samples: all,
  }
}

export function evaluationRegression(runId, baselineRunId) {
  const current = evaluationSummary(runId)
  const baseline = evaluationSummary(baselineRunId)
  if (!current || !baseline) return undefined
  const view = {
    run_id: runId,
    baseline_run_id: baselineRunId,
    flipped: [],
    newly_green: [],
    flaky: [],
    unchanged: 0,
    without_baseline: 0,
  }
  for (const cell of current.cells) {
    const before =
      baseline.cells.find(
        (item) => item.case_id === cell.case_id && item.scorer === cell.scorer
      ) ?? null
    const entry = {
      case_id: cell.case_id,
      scorer: cell.scorer,
      baseline: before,
      current: cell,
    }
    if (!before) view.without_baseline += 1
    if (cell.status === "flaky") view.flaky.push(entry)
    else if (before && before.status !== "red" && cell.status === "red")
      view.flipped.push(entry)
    else if (before && before.status !== "green" && cell.status === "green")
      view.newly_green.push(entry)
    else if (before) view.unchanged += 1
  }
  return view
}

const reasons = {
  "answers-the-question": [
    "The reply addresses what the customer asked.",
    "The customer asked for the price. The reply confirms the extension and never states a price.",
  ],
  "friendly-tone": [
    "Polite and to the point.",
    "The reply is curt and skips the acknowledgement the rubric asks for.",
  ],
  events: [
    "Every expected event was observed for the turn's correlation.",
    "Expected bike-rental/damage-reported once, observed 0 times.",
  ],
  state: [
    "The read model matches the expected state.",
    "Expected rented = true, read rented = false.",
  ],
}

export function evaluationSample(runId, caseId, epoch) {
  const run = runs.find((item) => item.run_id === runId)
  const item = cases.find((entry) => entry.name === caseId)
  if (!run?.outcomes || !item || ![1, 2].includes(epoch)) return undefined
  const recorded = verdicts(run, item, epoch).map((verdict) => ({
    scorer: verdict.scorer,
    ...(verdict.criterion ? { criterion: verdict.criterion } : {}),
    value: verdict.value,
    reasoning:
      reasons[verdict.criterion ?? verdict.scorer][
        verdict.value === "pass" ? 0 : 1
      ],
    ...(verdict.scorer === "conversation-quality"
      ? {
          metadata: {
            judge_model: "judge-model-pinned",
            grade: verdict.value === "pass" ? "C" : "I",
          },
        }
      : {}),
  }))
  // The state scorer was re-run for this sample: both verdicts stay recorded.
  if (
    runId === "nightly-2026-09-10" &&
    caseId === "rent-city-bike" &&
    epoch === 1
  )
    recorded.unshift({
      scorer: "state",
      value: "fail",
      reasoning: "The read model was not caught up when the scorer read it.",
    })
  return {
    run_id: runId,
    case_id: caseId,
    epoch,
    status: "completed",
    requested_model: "model-under-test",
    case: {
      name: item.name,
      description: item.description,
      steps: item.steps,
      plates: [],
    },
    turns: item.steps.map((step, index) => ({
      cid: item.cids[index],
      customer_message: step.message,
      reply: item.replies[index],
      model: "model-under-test",
      input_tokens: 1240,
      output_tokens: 96,
      latency_ms: 1850,
      prompt_fingerprint: "sha256:9f2b4c",
    })),
    verdicts: recorded,
    failure: null,
  }
}

function notFound(code, message) {
  return {
    status: 404,
    body: { error: { classification: "not_found", code, message } },
  }
}

// Answers POST <base>/contexts/evaluation/queries/<name>/schemas/1.
export function evaluationResponse(pathname, payload) {
  const name = /^\/contexts\/evaluation\/queries\/([^/]+)\/schemas\/1$/.exec(
    pathname
  )?.[1]
  const found = (body, code, message) =>
    body ? { status: 200, body } : notFound(code, message)
  if (name === "list-runs") return { status: 200, body: evaluationRuns() }
  if (name === "get-run-summary")
    return found(
      evaluationSummary(payload.run_id),
      "evaluation.run-not-found",
      "The eval run is unknown."
    )
  if (name === "get-regression")
    return evaluationSummary(payload.run_id)
      ? found(
          evaluationRegression(payload.run_id, payload.baseline_run_id),
          "evaluation.baseline-not-found",
          "The baseline run is unknown."
        )
      : notFound("evaluation.run-not-found", "The eval run is unknown.")
  if (name === "get-sample")
    return found(
      evaluationSample(payload.run_id, payload.case_id, payload.epoch),
      "evaluation.sample-not-found",
      "The sample is unknown."
    )
  return undefined
}
