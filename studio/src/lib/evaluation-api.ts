// Read-only client for a rostfrei evaluation application's query API.
// It is separate from the Tracer client: its own base URL and no Tracer token.

const EVALUATION_API_BASE = String(
  import.meta.env.VITE_EVAL_API_URL ?? ""
).replace(/\/$/, "")

export type EvaluationRunStatus = "started" | "rejected" | "completed"
export type EvaluationSampleStatus =
  "not_started" | "started" | "completed" | "failed"
export type EvaluationCellStatus = "green" | "red" | "flaky"
export type EvaluationVerdictValue = "pass" | "fail"

export interface EvaluationPlan {
  case_ids: string[]
  epochs: number
  requested_model: string
  commit: string
  prompt_reference: string
}

export interface EvaluationSampleCounts {
  planned: number
  started: number
  finished: number
  failed: number
}

export interface EvaluationRunOverview {
  run_id: string
  status: EvaluationRunStatus
  requested_at: string | null
  plan: EvaluationPlan | null
  sample_counts: EvaluationSampleCounts
}

export interface EvaluationCell {
  case_id: string
  scorer: string
  passed: number
  scored: number
  pass_rate: number
  status: EvaluationCellStatus
}

export interface EvaluationSampleSummary {
  case_id: string
  epoch: number
  status: EvaluationSampleStatus
  turns: number
  input_tokens: number
  output_tokens: number
  latency_ms: number
  failure: string | null
  verdicts: {
    scorer: string
    criterion: string | null
    value: EvaluationVerdictValue
  }[]
}

export interface EvaluationFinding {
  kind: string
  case_id?: string
  detail: string
}

export interface EvaluationRunSummary extends EvaluationRunOverview {
  findings: EvaluationFinding[]
  totals: {
    turns: number
    input_tokens: number
    output_tokens: number
    latency_ms: number
    mean_turn_latency_ms: number
  }
  cells: EvaluationCell[]
  samples: EvaluationSampleSummary[]
}

export interface EvaluationRegressionCell {
  case_id: string
  scorer: string
  baseline: EvaluationCell | null
  current: EvaluationCell
}

export interface EvaluationRegression {
  run_id: string
  baseline_run_id: string
  unchanged: number
  without_baseline: number
  flipped: EvaluationRegressionCell[]
  newly_green: EvaluationRegressionCell[]
  flaky: EvaluationRegressionCell[]
}

export interface EvaluationTurn {
  cid: string
  customer_message: string
  reply: string
  model: string
  input_tokens: number
  output_tokens: number
  latency_ms: number
  prompt_fingerprint: string
}

export interface EvaluationVerdict {
  scorer: string
  criterion?: string | null
  value: EvaluationVerdictValue
  reasoning: string
  metadata?: unknown
}

export interface EvaluationCase {
  name?: string
  description?: string
  steps?: { message?: string; referenceReply?: string }[]
  [key: string]: unknown
}

export interface EvaluationSample {
  run_id: string
  case_id: string
  epoch: number
  status: EvaluationSampleStatus
  requested_model: string
  case: EvaluationCase
  turns: EvaluationTurn[]
  verdicts: EvaluationVerdict[]
  failure: string | null
}

export type EvaluationFailure =
  | "run-not-found"
  | "baseline-not-found"
  | "sample-not-found"
  | "unavailable"
  | "request-failed"

export class EvaluationResponseError extends Error {
  readonly status: number
  readonly code?: string

  constructor(message: string, status: number, code?: string) {
    super(message)
    this.status = status
    this.code = code
  }

  override name = "EvaluationResponseError"
}

export function evaluationConfigured(): boolean {
  return Boolean(import.meta.env.VITE_EVAL_API_URL)
}

export function evaluationFailure(error: unknown): EvaluationFailure {
  if (!(error instanceof EvaluationResponseError)) return "request-failed"
  switch (error.code) {
    case "evaluation.run-not-found":
      return "run-not-found"
    case "evaluation.baseline-not-found":
      return "baseline-not-found"
    case "evaluation.sample-not-found":
      return "sample-not-found"
  }
  return error.status === 502 || error.status === 503 || error.status === 504
    ? "unavailable"
    : "request-failed"
}

export async function listEvaluationRuns(
  signal: AbortSignal
): Promise<EvaluationRunOverview[]> {
  const body = await query("list-runs", {}, signal)
  if (!Array.isArray(body.runs)) throw invalid("list-runs")
  return body.runs as EvaluationRunOverview[]
}

export async function getEvaluationRun(
  runId: string,
  signal: AbortSignal
): Promise<EvaluationRunSummary> {
  const body = await query("get-run-summary", { run_id: runId }, signal)
  if (!Array.isArray(body.cells) || !Array.isArray(body.samples))
    throw invalid("get-run-summary")
  return {
    ...(body as unknown as EvaluationRunSummary),
    findings: Array.isArray(body.findings) ? body.findings : [],
  }
}

export async function getEvaluationRegression(
  runId: string,
  baselineRunId: string,
  signal: AbortSignal
): Promise<EvaluationRegression> {
  const body = await query(
    "get-regression",
    { run_id: runId, baseline_run_id: baselineRunId },
    signal
  )
  if (
    !Array.isArray(body.flipped) ||
    !Array.isArray(body.newly_green) ||
    !Array.isArray(body.flaky)
  )
    throw invalid("get-regression")
  return body as unknown as EvaluationRegression
}

export async function getEvaluationSample(
  runId: string,
  caseId: string,
  epoch: number,
  signal: AbortSignal
): Promise<EvaluationSample> {
  const body = await query(
    "get-sample",
    { run_id: runId, case_id: caseId, epoch },
    signal
  )
  if (!Array.isArray(body.turns) || !Array.isArray(body.verdicts))
    throw invalid("get-sample")
  return {
    ...(body as unknown as EvaluationSample),
    case:
      typeof body.case === "object" && body.case !== null
        ? (body.case as EvaluationCase)
        : {},
  }
}

function queryPath(name: string): string {
  return `${EVALUATION_API_BASE}/contexts/evaluation/queries/${name}/schemas/1`
}

function label(name: string): string {
  return `POST ${new URL(queryPath(name), window.location.origin).pathname}`
}

function invalid(name: string): Error {
  return new Error(
    `${label(name)}: The evaluation API returned an unexpected response.`
  )
}

async function query(
  name: string,
  payload: Record<string, unknown>,
  signal: AbortSignal
): Promise<Record<string, unknown>> {
  const timeout = AbortSignal.timeout(15_000)
  let response: Response
  try {
    response = await fetch(queryPath(name), {
      method: "POST",
      signal: AbortSignal.any([signal, timeout]),
      redirect: "error",
      headers: {
        accept: "application/json",
        "content-type": "application/json",
      },
      body: JSON.stringify(payload),
    })
  } catch {
    throw new Error(
      `${label(name)}: ${timeout.aborted ? "The evaluation API did not respond before the request timed out." : "Could not reach the evaluation API. Check the server, VITE_EVAL_API_URL and VITE_EVAL_TARGET."}`
    )
  }
  let body: unknown
  try {
    body = await response.json()
  } catch {
    body = undefined
  }
  const value =
    typeof body === "object" && body !== null && !Array.isArray(body)
      ? (body as Record<string, unknown>)
      : undefined
  if (!response.ok) {
    // Application errors arrive as { error: { code, message } }; transport
    // errors of the rostfrei HTTP adapter as { code, message }.
    const detail = (
      typeof value?.error === "object" && value.error !== null
        ? value.error
        : (value ?? {})
    ) as { code?: unknown; message?: unknown }
    const code = typeof detail.code === "string" ? detail.code : undefined
    const text =
      response.status === 502 || response.status === 504
        ? "The Studio proxy could not reach the evaluation API. Check VITE_EVAL_TARGET and that the evaluation application is running."
        : typeof detail.message === "string"
          ? detail.message
          : code
    throw new EvaluationResponseError(
      `${label(name)}: ${response.status} ${response.statusText}${text ? `: ${text}` : ""}`,
      response.status,
      code
    )
  }
  if (!value) throw invalid(name)
  return value
}
