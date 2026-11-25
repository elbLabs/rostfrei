import {
  ArrowLeft,
  Check,
  CircleDashed,
  ClipboardCheck,
  LoaderCircle,
  Minus,
  RefreshCw,
  TriangleAlert,
  Unplug,
  Waypoints,
  X,
} from "lucide-react"
import { Badge } from "@rostfrei/ui/components/badge"
import { NativeSelect } from "@rostfrei/ui/components/native-select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@rostfrei/ui/components/table"
import { EvidenceDisclosure, EvidenceSummary } from "@rostfrei/ui/evidence"

import { Button } from "@/components/ui/button"
import { correlationHref } from "@/lib/deep-link"
import {
  evaluationMatrix,
  formatCount,
  formatDuration,
  groupVerdicts,
  scorerOutcome,
} from "@/lib/evaluation"
import type {
  EvaluationCell,
  EvaluationFailure,
  EvaluationRegression,
  EvaluationRegressionCell,
  EvaluationRunOverview,
  EvaluationRunSummary,
  EvaluationSample,
  EvaluationSampleSummary,
  EvaluationVerdictValue,
} from "@/lib/evaluation-api"
import type { EvaluationProblem, EvaluationState } from "@/lib/use-evaluation"
import { cn } from "@/lib/utils"

export function EvaluationSidebar({
  open,
  state,
  onClose,
  onSelect,
}: {
  open: boolean
  state: EvaluationState
  onClose: () => void
  onSelect: () => void
}) {
  const { runs, selectedRunId, connection } = state
  return (
    <aside
      id="studio-evaluation-panel"
      className={cn("studio-sidebar", open && "studio-sidebar-open")}
      inert={!open}
      aria-label="Eval runs"
    >
      <div className="sidebar-section-heading">
        <h2>
          <ClipboardCheck size={15} /> Eval runs <span>{runs.length}</span>
        </h2>
        <Button
          className="sidebar-close"
          variant="ghost"
          size="icon"
          onClick={onClose}
          aria-label="Close eval runs"
        >
          <X />
        </Button>
      </div>
      <div className="past-runs-scroll">
        {runs.map((run) => (
          <button
            type="button"
            key={run.run_id}
            data-eval-run={run.run_id}
            className={cn(
              "run-row",
              selectedRunId === run.run_id && "run-row-selected"
            )}
            aria-current={selectedRunId === run.run_id ? "page" : undefined}
            onClick={() => {
              state.selectRun(run.run_id)
              onSelect()
            }}
          >
            <RunStatusMark status={run.status} />
            <span>
              <span className="run-name">{run.run_id}</span>
              <span className="run-time">
                {formatRequestedAt(run.requested_at)} · {sampleProgress(run)}
              </span>
              {run.plan && (
                <span className="run-time">{run.plan.requested_model}</span>
              )}
            </span>
          </button>
        ))}
        {!runs.length && (
          <div className="sidebar-empty">
            <ClipboardCheck size={20} />
            <p>
              {connection === "connecting"
                ? "Loading eval runs…"
                : connection === "disconnected"
                  ? "The evaluation API could not be read."
                  : "No eval runs recorded yet."}
            </p>
            {connection === "live" && (
              <span>Start a run in the evaluation application.</span>
            )}
          </div>
        )}
      </div>
      <footer className="sidebar-footer">
        <span>Read from the evaluation API, newest request first.</span>
      </footer>
    </aside>
  )
}

export function EvaluationView({ state }: { state: EvaluationState }) {
  const { connection, runs, selectedRunId } = state
  if (connection === "disconnected")
    return (
      <section
        className="connection-notice"
        data-connection="evaluation-disconnected"
        role="alert"
      >
        <div className="connection-notice-heading">
          <Unplug size={20} />
          <div>
            <h1>Evaluation API connection failed</h1>
            <p>
              Eval runs are read from the application behind{" "}
              <code>VITE_EVAL_API_URL</code>. The development server proxies a
              relative URL to <code>VITE_EVAL_TARGET</code>.
            </p>
          </div>
          <Button className="connection-retry" onClick={state.refresh}>
            <RefreshCw /> Retry connection
          </Button>
        </div>
        <p className="connection-error-detail">
          {state.error ?? "The evaluation API could not be loaded."}
        </p>
      </section>
    )
  if (connection === "connecting" && !runs.length)
    return (
      <div className="evaluation" aria-busy="true">
        <p className="evaluation-status" role="status">
          <LoaderCircle size={16} className="animate-spin" /> Loading eval runs…
        </p>
      </div>
    )
  if (!selectedRunId)
    return (
      <div className="evaluation">
        <div className="evaluation-empty" role="status">
          <ClipboardCheck size={24} />
          <h1>No eval runs yet</h1>
          <p>
            Runs started in the evaluation application appear here with their
            samples, verdicts and regressions.
          </p>
        </div>
      </div>
    )
  if (state.sampleRef)
    return (
      <div
        className="evaluation"
        data-eval-view="sample"
        aria-busy={state.sampleLoading}
      >
        <Button
          className="evaluation-back"
          variant="ghost"
          onClick={state.closeSample}
        >
          <ArrowLeft /> Back to run
        </Button>
        {state.sampleProblem ? (
          <ProblemNotice problem={state.sampleProblem} />
        ) : state.sample ? (
          <SampleDetail sample={state.sample} />
        ) : (
          <p className="evaluation-status" role="status">
            <LoaderCircle size={16} className="animate-spin" /> Loading{" "}
            {state.sampleRef.caseId}, epoch {state.sampleRef.epoch}…
          </p>
        )}
      </div>
    )
  return (
    <div
      className="evaluation"
      data-eval-view="run"
      aria-busy={state.runLoading}
    >
      {state.runProblem ? (
        <ProblemNotice problem={state.runProblem} />
      ) : state.run ? (
        <RunDetail run={state.run} state={state} />
      ) : (
        <p className="evaluation-status" role="status">
          <LoaderCircle size={16} className="animate-spin" /> Loading{" "}
          {selectedRunId}…
        </p>
      )}
    </div>
  )
}

function RunDetail({
  run,
  state,
}: {
  run: EvaluationRunSummary
  state: EvaluationState
}) {
  const counts = run.sample_counts
  return (
    <>
      <EvidenceSummary
        title={run.run_id}
        facts={[
          {
            label: "Run",
            value: runStatusLabel(run.status),
            tone:
              run.status === "completed"
                ? "positive"
                : run.status === "rejected"
                  ? "negative"
                  : "warning",
          },
          {
            label: "Samples",
            value: `${counts.finished} of ${counts.planned} finished`,
            tone: counts.finished < counts.planned ? "warning" : "neutral",
          },
          ...(counts.failed
            ? [
                {
                  label: "Failed samples",
                  value: String(counts.failed),
                  tone: "negative" as const,
                },
              ]
            : []),
          { label: "Turns", value: formatCount(run.totals.turns) },
          {
            label: "Tokens",
            value: `${formatCount(run.totals.input_tokens)} in, ${formatCount(run.totals.output_tokens)} out`,
          },
          {
            label: "Mean turn latency",
            value: formatDuration(run.totals.mean_turn_latency_ms),
          },
        ]}
      >
        <dl className="evaluation-plan">
          <div>
            <dt>Requested</dt>
            <dd>{formatRequestedAt(run.requested_at)}</dd>
          </div>
          {run.plan && (
            <>
              <div>
                <dt>Model</dt>
                <dd>{run.plan.requested_model}</dd>
              </div>
              <div>
                <dt>Commit</dt>
                <dd>
                  <code>{run.plan.commit}</code>
                </dd>
              </div>
              <div>
                <dt>Prompt</dt>
                <dd>
                  <code>{run.plan.prompt_reference}</code>
                </dd>
              </div>
            </>
          )}
        </dl>
      </EvidenceSummary>

      {run.findings.length > 0 && (
        <section
          className="evaluation-section"
          aria-labelledby="evaluation-findings"
        >
          <h2 id="evaluation-findings">Admission findings</h2>
          <p className="evaluation-note">
            The run was rejected before any sample started.
          </p>
          <ul className="evaluation-findings">
            {run.findings.map((finding, index) => (
              <li key={index}>
                <Badge variant="destructive">{finding.kind}</Badge>
                {finding.case_id && <strong>{finding.case_id}</strong>}
                <span>{finding.detail}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {run.status !== "rejected" && <RunResults run={run} state={state} />}
    </>
  )
}

function RunResults({
  run,
  state,
}: {
  run: EvaluationRunSummary
  state: EvaluationState
}) {
  const matrix = evaluationMatrix(run)
  return (
    <>
      <section
        className="evaluation-section"
        aria-labelledby="evaluation-matrix"
      >
        <h2 id="evaluation-matrix">Cases by epoch</h2>
        {matrix.rows.length ? (
          <>
            <p className="evaluation-note">
              One sample per case and epoch. Open a sample for its transcript
              and verdicts.
              {matrix.scorers.length > 0 &&
                ` Marks follow the scorer order: ${matrix.scorers.join(", ")}.`}
            </p>
            <Table className="evaluation-matrix">
              <TableHeader>
                <TableRow>
                  <TableHead scope="col">Case</TableHead>
                  {matrix.epochs.map((epoch) => (
                    <TableHead key={epoch} scope="col">
                      Epoch {epoch}
                    </TableHead>
                  ))}
                  {matrix.scorers.map((scorer) => (
                    <TableHead key={scorer} scope="col">
                      {scorer}
                    </TableHead>
                  ))}
                </TableRow>
              </TableHeader>
              <TableBody>
                {matrix.rows.map((row) => (
                  <TableRow key={row.caseId} data-eval-case={row.caseId}>
                    <TableHead scope="row">{row.caseId}</TableHead>
                    {row.samples.map((sample, index) => (
                      <TableCell
                        key={matrix.epochs[index]}
                        data-label={`Epoch ${matrix.epochs[index]}`}
                      >
                        {sample ? (
                          <SampleButton
                            sample={sample}
                            scorers={matrix.scorers}
                            onOpen={() =>
                              state.openSample({
                                caseId: sample.case_id,
                                epoch: sample.epoch,
                              })
                            }
                          />
                        ) : (
                          <span className="evaluation-gap">Not started</span>
                        )}
                      </TableCell>
                    ))}
                    {row.cells.map((cell, index) => (
                      <TableCell
                        key={matrix.scorers[index]}
                        data-label={matrix.scorers[index]}
                      >
                        {cell ? (
                          <CellBadge cell={cell} />
                        ) : (
                          <span className="evaluation-gap">Not scored</span>
                        )}
                      </TableCell>
                    ))}
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            <p className="evaluation-legend">
              <Badge variant="success">green</Badge> every scored epoch passed
              <Badge variant="destructive">red</Badge> none passed
              <Badge variant="warning">flaky</Badge> some passed
            </p>
          </>
        ) : (
          <p className="evaluation-note">This run has no samples.</p>
        )}
      </section>

      <section
        className="evaluation-section"
        aria-labelledby="evaluation-regression"
        aria-busy={state.regressionLoading}
      >
        <h2 id="evaluation-regression">Regression</h2>
        <label className="evaluation-baseline">
          Baseline run
          <NativeSelect
            aria-label="Baseline run"
            value={state.baselineRunId ?? ""}
            onChange={(event) =>
              state.selectBaseline(event.target.value || undefined)
            }
          >
            <option value="">No baseline</option>
            {state.runs
              .filter((entry) => entry.run_id !== run.run_id)
              .map((entry) => (
                <option key={entry.run_id} value={entry.run_id}>
                  {entry.run_id} ({runStatusLabel(entry.status).toLowerCase()},{" "}
                  {formatRequestedAt(entry.requested_at)})
                </option>
              ))}
          </NativeSelect>
        </label>
        {state.regressionProblem ? (
          <ProblemNotice problem={state.regressionProblem} />
        ) : state.regression ? (
          <RegressionDetail regression={state.regression} />
        ) : state.baselineRunId ? (
          <p className="evaluation-status" role="status">
            <LoaderCircle size={16} className="animate-spin" /> Comparing with{" "}
            {state.baselineRunId}…
          </p>
        ) : (
          <p className="evaluation-note">
            Choose a baseline run to see which cells flipped, turned green or
            are flaky.
          </p>
        )}
      </section>
    </>
  )
}

function SampleButton({
  sample,
  scorers,
  onOpen,
}: {
  sample: EvaluationSampleSummary
  scorers: string[]
  onOpen: () => void
}) {
  const outcomes = scorers.map(
    (scorer) => [scorer, scorerOutcome(sample.verdicts, scorer)] as const
  )
  const state =
    sample.status === "failed"
      ? "Sample failed"
      : sample.status === "started"
        ? "Running"
        : undefined
  return (
    <button
      type="button"
      className="evaluation-sample"
      data-eval-sample={`${sample.case_id}#${sample.epoch}`}
      data-status={sample.status}
      aria-label={`${sample.case_id}, epoch ${sample.epoch}: ${[
        ...(state ? [state.toLowerCase()] : []),
        ...outcomes.map(
          ([scorer, outcome]) => `${scorer} ${outcome ?? "not scored"}`
        ),
      ].join(", ")}`}
      onClick={onOpen}
    >
      {outcomes.map(([scorer, outcome]) => (
        <span
          key={scorer}
          className="evaluation-pip"
          data-scorer={scorer}
          data-outcome={outcome ?? "none"}
          title={`${scorer}: ${outcome ?? "not scored"}`}
        >
          {outcome === "pass" ? (
            <Check />
          ) : outcome === "fail" ? (
            <X />
          ) : (
            <Minus />
          )}
        </span>
      ))}
      {sample.status === "failed" && <TriangleAlert size={14} />}
      {sample.status === "started" && <CircleDashed size={14} />}
      {state && <span className="evaluation-sample-state">{state}</span>}
    </button>
  )
}

function CellBadge({ cell }: { cell: EvaluationCell }) {
  return (
    <Badge
      data-eval-cell={`${cell.case_id}#${cell.scorer}`}
      data-status={cell.status}
      variant={
        cell.status === "green"
          ? "success"
          : cell.status === "red"
            ? "destructive"
            : "warning"
      }
    >
      {cell.passed}/{cell.scored} {cell.status}
    </Badge>
  )
}

function RegressionDetail({
  regression,
}: {
  regression: EvaluationRegression
}) {
  const changed =
    regression.flipped.length +
    regression.newly_green.length +
    regression.flaky.length
  return (
    <div data-eval-regression-baseline={regression.baseline_run_id}>
      <dl className="rf-evidence-facts">
        <div data-tone={regression.flipped.length ? "negative" : "neutral"}>
          <dt>Flipped to red</dt>
          <dd>{regression.flipped.length}</dd>
        </div>
        <div data-tone={regression.newly_green.length ? "positive" : "neutral"}>
          <dt>Newly green</dt>
          <dd>{regression.newly_green.length}</dd>
        </div>
        <div data-tone={regression.flaky.length ? "warning" : "neutral"}>
          <dt>Flaky</dt>
          <dd>{regression.flaky.length}</dd>
        </div>
        <div data-tone="neutral">
          <dt>Unchanged</dt>
          <dd>{regression.unchanged}</dd>
        </div>
        <div data-tone="neutral">
          <dt>Without baseline</dt>
          <dd>{regression.without_baseline}</dd>
        </div>
      </dl>
      {changed === 0 && (
        <p className="evaluation-note">
          No cell changed against {regression.baseline_run_id}.
        </p>
      )}
      <RegressionList
        kind="flipped"
        title="Flipped to red"
        entries={regression.flipped}
      />
      <RegressionList
        kind="newly-green"
        title="Newly green"
        entries={regression.newly_green}
      />
      <RegressionList kind="flaky" title="Flaky" entries={regression.flaky} />
    </div>
  )
}

function RegressionList({
  kind,
  title,
  entries,
}: {
  kind: string
  title: string
  entries: EvaluationRegressionCell[]
}) {
  if (!entries.length) return null
  return (
    <div className="evaluation-regression-group" data-eval-regression={kind}>
      <h3>{title}</h3>
      <ul>
        {entries.map((entry) => (
          <li key={`${entry.case_id}\n${entry.scorer}`}>
            <span className="evaluation-regression-subject">
              <strong>{entry.case_id}</strong>
              <span>{entry.scorer}</span>
            </span>
            <span className="evaluation-regression-change">
              {entry.baseline ? (
                <CellBadge cell={entry.baseline} />
              ) : (
                <Badge variant="neutral">no baseline</Badge>
              )}
              <span aria-label="now">→</span>
              <CellBadge cell={entry.current} />
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}

function SampleDetail({ sample }: { sample: EvaluationSample }) {
  const groups = groupVerdicts(sample.verdicts)
  const tokens = sample.turns.reduce(
    (total, turn) => ({
      input: total.input + turn.input_tokens,
      output: total.output + turn.output_tokens,
      latency: total.latency + turn.latency_ms,
    }),
    { input: 0, output: 0, latency: 0 }
  )
  return (
    <>
      <EvidenceSummary
        title={`${sample.case_id}, epoch ${sample.epoch}`}
        facts={[
          {
            label: "Sample",
            value:
              sample.status === "completed"
                ? "Completed"
                : sample.status === "failed"
                  ? "Failed"
                  : "Running",
            tone:
              sample.status === "completed"
                ? "neutral"
                : sample.status === "failed"
                  ? "negative"
                  : "warning",
          },
          ...groups.map((group) => ({
            label: group.scorer,
            value: group.outcome === "pass" ? "Pass" : "Fail",
            tone:
              group.outcome === "pass"
                ? ("positive" as const)
                : ("negative" as const),
          })),
          { label: "Requested model", value: sample.requested_model },
          {
            label: "Tokens",
            value: `${formatCount(tokens.input)} in, ${formatCount(tokens.output)} out`,
          },
          { label: "Latency", value: formatDuration(tokens.latency) },
        ]}
      >
        <p className="evaluation-note">
          Run <code>{sample.run_id}</code>
          {sample.case.description ? `. ${sample.case.description}` : ""}
        </p>
      </EvidenceSummary>
      {sample.failure && (
        <p className="evaluation-failure" role="alert">
          <TriangleAlert size={16} /> {sample.failure}
        </p>
      )}

      <section
        className="evaluation-section"
        aria-labelledby="evaluation-transcript"
      >
        <h2 id="evaluation-transcript">Transcript</h2>
        {sample.turns.length ? (
          <ol className="evaluation-transcript">
            {sample.turns.map((turn, index) => {
              const reference = sample.case.steps?.[index]?.referenceReply
              return (
                <li key={`${turn.cid}-${index}`} data-eval-turn={index + 1}>
                  <header>
                    <h3>Turn {index + 1}</h3>
                    <span>
                      {turn.model} · {formatCount(turn.input_tokens)} in,{" "}
                      {formatCount(turn.output_tokens)} out ·{" "}
                      {formatDuration(turn.latency_ms)}
                    </span>
                  </header>
                  <div className="evaluation-message" data-speaker="customer">
                    <span>Customer</span>
                    <p>{turn.customer_message}</p>
                  </div>
                  <div className="evaluation-message" data-speaker="reply">
                    <span>Reply</span>
                    <p>{turn.reply}</p>
                  </div>
                  {reference && (
                    <EvidenceDisclosure title="Reference reply">
                      <p className="evaluation-reference">{reference}</p>
                    </EvidenceDisclosure>
                  )}
                  <p className="evaluation-trace">
                    <a
                      href={correlationHref(turn.cid)}
                      data-eval-cid={turn.cid}
                    >
                      <Waypoints size={14} /> Open causal graph
                    </a>
                    <code title="Correlation ID">{turn.cid}</code>
                    <code title="Prompt fingerprint">
                      {turn.prompt_fingerprint}
                    </code>
                  </p>
                </li>
              )
            })}
          </ol>
        ) : (
          <p className="evaluation-note">No turns were recorded.</p>
        )}
      </section>

      <section
        className="evaluation-section"
        aria-labelledby="evaluation-verdicts"
      >
        <h2 id="evaluation-verdicts">Verdicts</h2>
        {groups.length ? (
          groups.map((group) => (
            <div
              key={group.scorer}
              className="evaluation-scorer"
              data-eval-scorer={group.scorer}
              data-outcome={group.outcome}
            >
              <h3>
                {group.scorer} <OutcomeBadge value={group.outcome} />
              </h3>
              <ul>
                {group.verdicts.map(({ verdict, superseded }, index) => (
                  <li key={index} data-superseded={superseded || undefined}>
                    <p className="evaluation-criterion">
                      <OutcomeBadge value={verdict.value} />
                      <strong>{verdict.criterion ?? "Overall"}</strong>
                      {superseded && (
                        <Badge variant="neutral">superseded</Badge>
                      )}
                    </p>
                    <p className="evaluation-reasoning">{verdict.reasoning}</p>
                    {verdict.metadata !== undefined &&
                      verdict.metadata !== null && (
                        <EvidenceDisclosure title="Scorer metadata">
                          <pre>{JSON.stringify(verdict.metadata, null, 2)}</pre>
                        </EvidenceDisclosure>
                      )}
                  </li>
                ))}
              </ul>
            </div>
          ))
        ) : (
          <p className="evaluation-note">
            {sample.status === "started"
              ? "Scorers run after the sample completes."
              : "No verdict was recorded for this sample."}
          </p>
        )}
      </section>

      <EvidenceDisclosure title="Case definition">
        <pre>{JSON.stringify(sample.case, null, 2)}</pre>
      </EvidenceDisclosure>
    </>
  )
}

function OutcomeBadge({ value }: { value: EvaluationVerdictValue }) {
  return (
    <Badge variant={value === "pass" ? "success" : "destructive"}>
      {value === "pass" ? <Check /> : <X />}
      {value}
    </Badge>
  )
}

function ProblemNotice({ problem }: { problem: EvaluationProblem }) {
  return (
    <div
      className="evaluation-problem"
      data-eval-failure={problem.failure}
      role="alert"
    >
      <strong>{failureTitle(problem.failure)}</strong>
      <span>{failureDetail(problem.failure)}</span>
      <code>{problem.message}</code>
    </div>
  )
}

function RunStatusMark({
  status,
}: {
  status: EvaluationRunOverview["status"]
}) {
  return (
    <span
      className={cn(
        "run-status",
        status === "completed"
          ? "run-status-pass"
          : status === "rejected"
            ? "run-status-fail"
            : "run-status-pending"
      )}
      aria-label={runStatusLabel(status)}
    >
      {status === "completed" ? (
        <Check />
      ) : status === "rejected" ? (
        <X />
      ) : (
        <CircleDashed />
      )}
    </span>
  )
}

function runStatusLabel(status: EvaluationRunOverview["status"]): string {
  return status === "completed"
    ? "Completed"
    : status === "rejected"
      ? "Rejected"
      : "Running"
}

function sampleProgress(run: EvaluationRunOverview): string {
  const { finished, planned, failed } = run.sample_counts
  return `${finished}/${planned} samples${failed ? `, ${failed} failed` : ""}`
}

function formatRequestedAt(value: string | null): string {
  const date = value ? new Date(value) : undefined
  return !date || Number.isNaN(date.getTime())
    ? "Unknown time"
    : new Intl.DateTimeFormat(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      }).format(date)
}

function failureTitle(failure: EvaluationFailure): string {
  switch (failure) {
    case "run-not-found":
      return "Eval run not found"
    case "baseline-not-found":
      return "Baseline run not found"
    case "sample-not-found":
      return "Sample not found"
    case "unavailable":
      return "Evaluation results unavailable"
    case "request-failed":
      return "Evaluation request failed"
  }
}

function failureDetail(failure: EvaluationFailure): string {
  switch (failure) {
    case "run-not-found":
      return "The evaluation application does not know this run. Refresh the run list."
    case "baseline-not-found":
      return "The chosen baseline is unknown to the evaluation application. Choose another run."
    case "sample-not-found":
      return "This case and epoch has not started in the run."
    case "unavailable":
      return "The evaluation application could not read its results. This is not an empty result; refresh to retry."
    case "request-failed":
      return "The request did not complete. Check the evaluation API connection and refresh."
  }
}
