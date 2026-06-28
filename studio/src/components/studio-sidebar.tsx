import { Check, FlaskConical, History, X } from "lucide-react"

import { Button } from "@/components/ui/button"
import type { StoredRun, TestDefinitionSummary } from "@/lib/types"
import { cn } from "@/lib/utils"

interface StudioSidebarProps {
  open: boolean
  tests: TestDefinitionSummary[]
  selectedTestId?: string
  selectedRunId?: string
  runs: StoredRun[]
  busy: boolean
  storageNote?: string
  onClose: () => void
  onSelectTest: (test: TestDefinitionSummary) => void
  onSelectRun: (run: StoredRun) => void
}

export function StudioSidebar({
  open,
  tests,
  selectedTestId,
  selectedRunId,
  runs,
  busy,
  storageNote,
  onClose,
  onSelectTest,
  onSelectRun,
}: StudioSidebarProps) {
  return (
    <aside
      id="studio-navigation"
      className={cn("studio-sidebar", open && "studio-sidebar-open")}
      inert={!open}
      aria-label="Tests and run history"
    >
      <div className="sidebar-section-heading">
        <h2>
          <FlaskConical size={15} /> Tests <span>{tests.length}</span>
        </h2>
        <Button
          className="sidebar-close"
          variant="ghost"
          size="icon"
          onClick={onClose}
          aria-label="Close sidebar"
        >
          <X />
        </Button>
      </div>
      <nav className="test-list" aria-label="Behavioral tests">
        {tests.map((test) => (
          <button
            type="button"
            key={test.id}
            className={cn(
              "test-row",
              selectedTestId === test.id &&
                !selectedRunId &&
                "test-row-selected"
            )}
            aria-current={
              selectedTestId === test.id && !selectedRunId ? "page" : undefined
            }
            disabled={busy}
            onClick={() => onSelectTest(test)}
          >
            <FlaskConical size={15} />
            <span>{test.name}</span>
          </button>
        ))}
        {!tests.length && <p className="sidebar-empty">No tests registered.</p>}
      </nav>
      <div className="sidebar-section-heading">
        <h2>
          <History size={15} /> Recent runs <span>{runs.length}</span>
        </h2>
      </div>
      <div className="past-runs-scroll">
        {runs.length ? (
          runs.map((run) => (
            <button
              type="button"
              key={run.runId}
              className={cn(
                "run-row",
                selectedRunId === run.runId && "run-row-selected"
              )}
              aria-current={selectedRunId === run.runId ? "page" : undefined}
              disabled={busy}
              onClick={() => onSelectRun(run)}
            >
              <span
                className={cn(
                  "run-status",
                  run.status === "passed"
                    ? "run-status-pass"
                    : "run-status-fail"
                )}
                aria-label={run.status}
              >
                {run.status === "passed" ? <Check /> : <X />}
              </span>
              <span>
                <span className="run-name">{run.testName}</span>
                <span className="run-time">
                  {formatRunTime(run.createdAt)}
                  {(run.source === "demo" || run.runId.startsWith("demo-")) &&
                    " · Demo"}
                </span>
              </span>
            </button>
          ))
        ) : (
          <div className="sidebar-empty">
            <History size={20} />
            <p>Your executions will appear here.</p>
            <span>Run a test to explore what happened.</span>
          </div>
        )}
      </div>
      <footer className="sidebar-footer">
        <span>History saved in this browser</span>
        {storageNote && <p role="status">{storageNote}</p>}
      </footer>
    </aside>
  )
}

function formatRunTime(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime())
    ? "Unknown time"
    : new Intl.DateTimeFormat(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      }).format(date)
}
