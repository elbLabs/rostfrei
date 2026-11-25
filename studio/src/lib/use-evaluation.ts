import { useCallback, useEffect, useState } from "react"

import {
  evaluationFailure,
  getEvaluationRegression,
  getEvaluationRun,
  getEvaluationSample,
  listEvaluationRuns,
  type EvaluationFailure,
  type EvaluationRegression,
  type EvaluationRunOverview,
  type EvaluationRunSummary,
  type EvaluationSample,
} from "@/lib/evaluation-api"

export type EvaluationConnection = "connecting" | "live" | "disconnected"

export interface EvaluationProblem {
  failure: EvaluationFailure
  message: string
}

export interface EvaluationSampleRef {
  caseId: string
  epoch: number
}

// A result remembers the request it answers, so a newer selection reads as
// loading instead of showing another run's data.
interface Loaded<T> {
  key: string
  value?: T
  problem?: EvaluationProblem
}

export interface EvaluationState {
  connection: EvaluationConnection
  error?: string
  runs: EvaluationRunOverview[]
  selectedRunId?: string
  run?: EvaluationRunSummary
  runLoading: boolean
  runProblem?: EvaluationProblem
  baselineRunId?: string
  regression?: EvaluationRegression
  regressionLoading: boolean
  regressionProblem?: EvaluationProblem
  sampleRef?: EvaluationSampleRef
  sample?: EvaluationSample
  sampleLoading: boolean
  sampleProblem?: EvaluationProblem
  selectRun: (runId: string) => void
  selectBaseline: (runId: string | undefined) => void
  openSample: (sample: EvaluationSampleRef) => void
  closeSample: () => void
  refresh: () => void
}

// The run requested before the selected one is the default comparison.
export function defaultBaseline(
  runs: EvaluationRunOverview[],
  runId: string | undefined
): string | undefined {
  const index = runs.findIndex((run) => run.run_id === runId)
  if (index < 0) return undefined
  return runs.slice(index + 1).find((run) => run.status === "completed")?.run_id
}

export function useEvaluation(active: boolean): EvaluationState {
  const [attempt, setAttempt] = useState(0)
  const [connection, setConnection] =
    useState<EvaluationConnection>("connecting")
  const [error, setError] = useState<string>()
  const [runs, setRuns] = useState<EvaluationRunOverview[]>([])
  const [chosenRunId, setChosenRunId] = useState<string>()
  // undefined follows the default baseline; null is an explicit "no baseline".
  const [baselineChoice, setBaselineChoice] = useState<string | null>()
  const [sampleRef, setSampleRef] = useState<EvaluationSampleRef>()
  const [run, setRun] = useState<Loaded<EvaluationRunSummary>>()
  const [regression, setRegression] = useState<Loaded<EvaluationRegression>>()
  const [sample, setSample] = useState<Loaded<EvaluationSample>>()

  const selectedRunId =
    runs.find((entry) => entry.run_id === chosenRunId)?.run_id ??
    runs[0]?.run_id
  // A rejected run never scored a sample, so it has nothing to compare.
  const comparable =
    runs.find((entry) => entry.run_id === selectedRunId)?.status !== "rejected"
  const baselineRunId =
    baselineChoice === null || !comparable
      ? undefined
      : (runs.find(
          (entry) =>
            entry.run_id === baselineChoice && entry.run_id !== selectedRunId
        )?.run_id ?? defaultBaseline(runs, selectedRunId))
  const runKey = selectedRunId
  const regressionKey =
    selectedRunId && baselineRunId
      ? `${selectedRunId}\n${baselineRunId}`
      : undefined
  const sampleKey =
    selectedRunId && sampleRef
      ? `${selectedRunId}\n${sampleRef.caseId}\n${sampleRef.epoch}`
      : undefined

  useEffect(() => {
    if (!active) return
    const controller = new AbortController()
    void listEvaluationRuns(controller.signal)
      .then((listed) => {
        if (controller.signal.aborted) return
        setRuns(listed)
        setError(undefined)
        setConnection("live")
      })
      .catch((failure: unknown) => {
        if (controller.signal.aborted) return
        setError(message(failure))
        setConnection("disconnected")
      })
    return () => controller.abort()
  }, [active, attempt])

  useEffect(() => {
    if (!active || !runKey) return
    const controller = new AbortController()
    void getEvaluationRun(runKey, controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) setRun({ key: runKey, value })
      })
      .catch((failure: unknown) => {
        if (!controller.signal.aborted)
          setRun({ key: runKey, problem: problem(failure) })
      })
    return () => controller.abort()
  }, [active, attempt, runKey])

  useEffect(() => {
    if (!active || !regressionKey) return
    const [runId, baselineId] = regressionKey.split("\n")
    const controller = new AbortController()
    void getEvaluationRegression(runId, baselineId, controller.signal)
      .then((value) => {
        if (!controller.signal.aborted)
          setRegression({ key: regressionKey, value })
      })
      .catch((failure: unknown) => {
        if (!controller.signal.aborted)
          setRegression({ key: regressionKey, problem: problem(failure) })
      })
    return () => controller.abort()
  }, [active, attempt, regressionKey])

  useEffect(() => {
    if (!active || !sampleKey) return
    const [runId, caseId, epoch] = sampleKey.split("\n")
    const controller = new AbortController()
    void getEvaluationSample(runId, caseId, Number(epoch), controller.signal)
      .then((value) => {
        if (!controller.signal.aborted) setSample({ key: sampleKey, value })
      })
      .catch((failure: unknown) => {
        if (!controller.signal.aborted)
          setSample({ key: sampleKey, problem: problem(failure) })
      })
    return () => controller.abort()
  }, [active, attempt, sampleKey])

  const selectRun = useCallback((runId: string) => {
    setChosenRunId(runId)
    setBaselineChoice(undefined)
    setSampleRef(undefined)
  }, [])
  const selectBaseline = useCallback((runId: string | undefined) => {
    setBaselineChoice(runId ?? null)
  }, [])
  const closeSample = useCallback(() => setSampleRef(undefined), [])
  const refresh = useCallback(() => {
    setConnection((current) =>
      current === "disconnected" ? "connecting" : current
    )
    setAttempt((current) => current + 1)
  }, [])

  const currentRun = run?.key === runKey ? run : undefined
  const currentRegression =
    regression?.key === regressionKey ? regression : undefined
  const currentSample = sample?.key === sampleKey ? sample : undefined
  return {
    connection,
    error,
    runs,
    selectedRunId,
    run: currentRun?.value,
    runLoading: Boolean(runKey) && !currentRun,
    runProblem: currentRun?.problem,
    baselineRunId,
    regression: currentRegression?.value,
    regressionLoading: Boolean(regressionKey) && !currentRegression,
    regressionProblem: currentRegression?.problem,
    sampleRef,
    sample: currentSample?.value,
    sampleLoading: Boolean(sampleKey) && !currentSample,
    sampleProblem: currentSample?.problem,
    selectRun,
    selectBaseline,
    openSample: setSampleRef,
    closeSample,
    refresh,
  }
}

function problem(failure: unknown): EvaluationProblem {
  return { failure: evaluationFailure(failure), message: message(failure) }
}

function message(failure: unknown): string {
  return failure instanceof Error
    ? failure.message
    : "The evaluation request failed"
}
