import { Check, LoaderCircle, TriangleAlert } from "lucide-react"

import type { FlowView, MessageGraphNode, StoredRun } from "@/lib/types"

export function ExecutionHeader({
  name,
  run,
  nodes,
  view,
  loading,
  canRun,
  connected,
  error,
}: {
  name?: string
  run?: StoredRun
  nodes: MessageGraphNode[]
  view: FlowView
  loading: boolean
  canRun: boolean
  connected: boolean
  error?: string
}) {
  const root =
    nodes.find((node) => node.subject === true) ??
    nodes.find(
      (node) =>
        node.kind === "command" && !node.context && node.subject !== false
    )
  const status = loading
    ? "loading"
    : view === "running"
      ? "running"
      : view === "unavailable"
        ? "unavailable"
        : run?.status
  const StatusIcon =
    status === "passed"
      ? Check
      : status === "loading" || status === "running"
        ? LoaderCircle
        : TriangleAlert
  const verdict =
    status === "loading"
      ? "Loading test"
      : status === "running"
        ? "Running test"
        : status === "unavailable"
          ? "Run result unavailable"
          : status === "passed"
            ? "Test passed"
            : "Test failed"
  const explanation = run
    ? root?.status === "accepted" || root?.status === "rejected"
      ? `Command ${root.status}${run.status === "passed" ? " as expected" : "; expectations were not met"}`
      : run.status === "passed"
        ? "Expected messages matched"
        : "Expectations were not met"
    : status === "unavailable"
      ? "No complete run report was received"
      : undefined

  if (!name && !loading && !error) return null

  return (
    <section className="execution-header" aria-label="Execution summary">
      <div className="execution-title-row">
        <div>
          <h1 title={name}>{name ?? "Loading test"}</h1>
        </div>
      </div>
      {status && (
        <div className="execution-result" data-status={status} role="status">
          <StatusIcon
            size={16}
            className={
              status === "running" || status === "loading"
                ? "animate-spin"
                : undefined
            }
          />
          <strong>{verdict}</strong>
          {explanation && <span>{explanation}</span>}
        </div>
      )}
      {run?.capture &&
        (run.capture.fidelity === "grouped" ||
          !run.capture.settled ||
          run.capture.note) && (
          <p className="execution-evidence">
            {run.capture.fidelity === "grouped"
              ? "Grouped capture"
              : "Exact capture"}
            , {run.capture.settled ? "settled" : "not settled"}.
            {run.capture.note && ` ${run.capture.note}`}
          </p>
        )}
      {run && !canRun && !loading && (
        <p className="execution-note">
          {connected
            ? "This saved run’s test is not available from the current connection."
            : "Viewing a saved run. Reconnect to Tracer to run tests."}
        </p>
      )}
      {error && (
        <div className="execution-error" role="alert">
          <TriangleAlert size={16} />
          <span>{error}</span>
        </div>
      )}
      {run?.diagnostics && run.diagnostics.length > 0 && (
        <details className="run-diagnostics">
          <summary>
            {run.diagnostics.length} expectation{" "}
            {run.diagnostics.length === 1 ? "mismatch" : "mismatches"}
          </summary>
          <ul>
            {run.diagnostics.map((diagnostic, index) => (
              <li key={index}>
                <strong>{diagnostic.code}</strong>
                <p>{diagnostic.message}</p>
                <code>{diagnostic.path}</code>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  )
}
