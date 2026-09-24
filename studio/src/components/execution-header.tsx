import { useId, useState } from "react"
import {
  Check,
  ChevronDown,
  CircleDot,
  LoaderCircle,
  Play,
  RotateCcw,
  TriangleAlert,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { messageFacts } from "@/lib/message-presentation"
import type {
  FlowView,
  MessageGraphNode,
  StoredRun,
  StudioLayout,
  TestDefinitionRevision,
} from "@/lib/types"

interface ExecutionHeaderProps {
  layout: StudioLayout
  name?: string
  definition?: TestDefinitionRevision
  run?: StoredRun
  nodes: MessageGraphNode[]
  view: FlowView
  loading: boolean
  canRun: boolean
  demo: boolean
  error?: string
  onRun: () => void
}

export function ExecutionHeader({
  layout,
  name,
  definition,
  run,
  nodes,
  view,
  loading,
  canRun,
  demo,
  error,
  onRun,
}: ExecutionHeaderProps) {
  const [scenarioOpen, setScenarioOpen] = useState(false)
  const scenarioId = useId()
  const canvas = layout === "canvas"
  const root = nodes.find((node) => node.kind === "command" && !node.context)
  const expectations = definition?.definition.expected.graphs[0]?.nodes ?? []
  const expected = expectations.find(
    (node) => node.kind === "command" && !node.parentKey
  )
  const outcome = expected?.kind === "command" ? expected.outcome : undefined
  const fixtureCount = nodes.filter((node) => node.context).length
  const fixtureNames = [
    ...new Set(nodes.filter((node) => node.context).map((node) => node.name)),
  ]
  const expectedEventCount = expectations.filter(
    (node) => node.kind !== "command"
  ).length
  const status = loading
    ? "loading"
    : view === "running"
      ? "running"
      : view === "unavailable"
        ? "unavailable"
        : (run?.status ?? "ready")
  const StatusIcon =
    status === "passed"
      ? Check
      : status === "failed" || status === "unavailable"
        ? TriangleAlert
        : status === "running" || status === "loading"
          ? LoaderCircle
          : CircleDot
  const verdict =
    status === "loading"
      ? "Loading test"
      : status === "running"
        ? "Running test"
        : status === "unavailable"
          ? "Run result unavailable"
          : status === "passed"
            ? "Test passed"
            : status === "failed"
              ? "Test failed"
              : "Ready to run"
  const explanation = run
    ? root?.status === "accepted" || root?.status === "rejected"
      ? `Command ${root.status}${run.status === "passed" ? " as expected" : " · expectations were not met"}`
      : run.status === "passed"
        ? "Expected messages matched"
        : "Expectations were not met"
    : view === "running"
      ? "Waiting for the execution to complete…"
      : view === "unavailable"
        ? "No complete run report was received"
        : "Showing the test definition and expected messages"

  return (
    <section className="execution-header" aria-label="Execution summary">
      <div className="execution-title-row">
        <div>
          <p className="eyebrow">
            Behavioral tests <span>/</span>{" "}
            {run ? "Run details" : "Test definition"}
          </p>
          <h1 title={name}>{name ?? "Explore domain behavior"}</h1>
        </div>
        <div className="execution-actions">
          {canvas && name && (
            <Button
              className="scenario-toggle"
              variant="ghost"
              aria-expanded={scenarioOpen}
              aria-controls={scenarioId}
              onClick={() => setScenarioOpen((open) => !open)}
            >
              Scenario{" "}
              <ChevronDown
                className={scenarioOpen ? "rotate-180" : undefined}
              />
            </Button>
          )}
          <Button className="run-button" disabled={!canRun} onClick={onRun}>
            {view === "running" ? (
              <LoaderCircle className="animate-spin" />
            ) : run ? (
              <RotateCcw />
            ) : (
              <Play />
            )}
            {view === "running"
              ? "Running…"
              : run
                ? run.source === (demo ? "demo" : "live")
                  ? "Run again"
                  : demo
                    ? "Run demo"
                    : "Run in Test"
                : demo
                  ? "Run demo"
                  : "Run test"}
          </Button>
        </div>
      </div>
      {name ? (
        <>
          <dl
            id={scenarioId}
            className="scenario-summary"
            hidden={canvas && !scenarioOpen}
          >
            <div>
              <dt>Given</dt>
              <dd title={definition?.definition.setup.fixture}>
                {definition?.definition.setup.fixture ??
                  `${fixtureCount} fixture ${fixtureCount === 1 ? "event" : "events"}`}
                {fixtureNames.length > 0 && (
                  <span>
                    {fixtureNames.slice(0, 2).join(" · ")}
                    {fixtureNames.length > 2
                      ? ` · +${fixtureNames.length - 2} more`
                      : ""}
                  </span>
                )}
              </dd>
            </div>
            <div>
              <dt>When</dt>
              <dd>
                {root?.name ?? expected?.name ?? "Loading command…"}
                {root && (
                  <span>
                    {messageFacts(root)
                      .map(([key, value]) => `${key}: ${value}`)
                      .join(" · ")}
                  </span>
                )}
              </dd>
            </div>
            <div>
              <dt>Then</dt>
              <dd>
                {outcome === "accepted"
                  ? "Command accepted"
                  : outcome
                    ? "Command rejected"
                    : "Recorded expectations"}
                {outcome && outcome !== "accepted" && (
                  <span>{outcome.rejected.code}</span>
                )}
                {outcome === "accepted" && (
                  <span>
                    {expectedEventCount} expected{" "}
                    {expectedEventCount === 1 ? "event" : "events"}
                  </span>
                )}
              </dd>
            </div>
          </dl>
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
            <span>{explanation}</span>
            {run && (!canvas || scenarioOpen) && (
              <time dateTime={run.createdAt}>{formatTime(run.createdAt)}</time>
            )}
          </div>
          {!demo && (
            <p className="execution-note">
              Runs use the current test definition and reset isolated Test state
              to its fixture.
            </p>
          )}
          {run && !canRun && !loading && (
            <p className="execution-note">
              This saved run’s test is not available from the current
              connection.
            </p>
          )}
        </>
      ) : (
        <p className="execution-note">
          No behavioral tests are registered. Registered tests will appear in
          the sidebar.
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

function formatTime(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime())
    ? "Unknown time"
    : new Intl.DateTimeFormat(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      }).format(date)
}
