import type {
  EvaluationCell,
  EvaluationRunSummary,
  EvaluationSampleSummary,
  EvaluationVerdict,
  EvaluationVerdictValue,
} from "@/lib/evaluation-api"

export interface EvaluationMatrixRow {
  caseId: string
  // Aligned with the matrix epochs and scorers; a gap is a missing sample or score.
  samples: (EvaluationSampleSummary | undefined)[]
  cells: (EvaluationCell | undefined)[]
}

export interface EvaluationMatrix {
  scorers: string[]
  epochs: number[]
  rows: EvaluationMatrixRow[]
}

// Cases keep the plan order and epochs count from one, so a run still in
// progress shows its missing samples as gaps.
export function evaluationMatrix(run: EvaluationRunSummary): EvaluationMatrix {
  const caseIds = unique([
    ...(run.plan?.case_ids ?? []),
    ...run.samples.map((sample) => sample.case_id),
    ...run.cells.map((cell) => cell.case_id),
  ])
  const last = Math.max(
    run.plan?.epochs ?? 0,
    ...run.samples.map((sample) => sample.epoch)
  )
  const epochs = Array.from(
    { length: Math.max(last, 0) },
    (_, index) => index + 1
  )
  const scorers = unique([
    ...run.cells.map((cell) => cell.scorer),
    ...run.samples.flatMap((sample) =>
      sample.verdicts.map((verdict) => verdict.scorer)
    ),
  ]).sort()
  return {
    scorers,
    epochs,
    rows: caseIds.map((caseId) => ({
      caseId,
      samples: epochs.map((epoch) =>
        run.samples.find(
          (sample) => sample.case_id === caseId && sample.epoch === epoch
        )
      ),
      cells: scorers.map((scorer) =>
        run.cells.find(
          (cell) => cell.case_id === caseId && cell.scorer === scorer
        )
      ),
    })),
  }
}

// A scorer passes a sample only when each of its criteria passes, matching how
// the evaluation application counts a scored sample.
export function scorerOutcome(
  verdicts: readonly { scorer: string; value: EvaluationVerdictValue }[],
  scorer: string
): EvaluationVerdictValue | undefined {
  const own = verdicts.filter((verdict) => verdict.scorer === scorer)
  if (!own.length) return undefined
  return own.every((verdict) => verdict.value === "pass") ? "pass" : "fail"
}

export interface EvaluationScorerVerdicts {
  scorer: string
  outcome: EvaluationVerdictValue
  verdicts: { verdict: EvaluationVerdict; superseded: boolean }[]
}

// Recorded verdicts are append-only. A later verdict for the same scorer and
// criterion replaces the earlier one in the score, and both stay visible.
export function groupVerdicts(
  verdicts: readonly EvaluationVerdict[]
): EvaluationScorerVerdicts[] {
  const identity = (verdict: EvaluationVerdict) =>
    `${verdict.scorer}\n${verdict.criterion ?? ""}`
  const latest = new Map(
    verdicts.map((verdict, index) => [identity(verdict), index])
  )
  return unique(verdicts.map((verdict) => verdict.scorer))
    .sort()
    .map((scorer) => {
      const entries = verdicts.flatMap((verdict, index) =>
        verdict.scorer === scorer
          ? [{ verdict, superseded: latest.get(identity(verdict)) !== index }]
          : []
      )
      return {
        scorer,
        outcome: entries.every(
          (entry) => entry.superseded || entry.verdict.value === "pass"
        )
          ? "pass"
          : "fail",
        verdicts: entries,
      }
    })
}

export function formatDuration(milliseconds: number): string {
  if (!Number.isFinite(milliseconds)) return "Unknown"
  return milliseconds < 1000
    ? `${Math.round(milliseconds)} ms`
    : `${(milliseconds / 1000).toFixed(milliseconds < 10_000 ? 1 : 0)} s`
}

export function formatCount(value: number): string {
  return Number.isFinite(value) ? value.toLocaleString("en-US") : "Unknown"
}

function unique(values: string[]): string[] {
  return [...new Set(values)]
}
