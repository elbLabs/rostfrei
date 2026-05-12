import {
  Check,
  CircleOff,
  FlaskConical,
  History,
  LoaderCircle,
  Play,
  RotateCcw,
  X,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import type { StoredRun, TestDefinitionSummary } from "@/lib/types"
import { cn } from "@/lib/utils"

interface StudioSidebarProps {
  open: boolean
  tests: TestDefinitionSummary[]
  selectedTestId?: string
  selectedRunId?: string
  runs: StoredRun[]
  source: "connecting" | "live" | "demo"
  running: boolean
  onClose: () => void
  onSelectTest: (test: TestDefinitionSummary) => void
  onSelectRun: (run: StoredRun) => void
  onRun: () => void
}

export function StudioSidebar({
  open,
  tests,
  selectedTestId,
  selectedRunId,
  runs,
  source,
  running,
  onClose,
  onSelectTest,
  onSelectRun,
  onRun,
}: StudioSidebarProps) {
  return (
    <aside className={cn("studio-sidebar", open && "studio-sidebar-open")}>
      <div className="flex h-full min-h-0 flex-col">
        <div className="flex items-center justify-between px-3 pt-4 pb-2">
          <SectionLabel icon={FlaskConical}>Tests</SectionLabel>
          <Button
            variant="ghost"
            size="icon-sm"
            className="md:hidden"
            onClick={onClose}
            aria-label="Close sidebar"
          >
            <X />
          </Button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col px-2 pb-3">
          <div className="shrink-0 space-y-0.5">
            {tests.map((test) => {
              const selected = test.id === selectedTestId && !selectedRunId
              return (
                <div
                  key={test.id}
                  className={cn(
                    "test-row group",
                    selected && "test-row-selected"
                  )}
                >
                  <button
                    type="button"
                    className="min-w-0 flex-1 py-2.5 pl-2.5 text-left"
                    onClick={() => onSelectTest(test)}
                  >
                    <span className="block truncate text-[12px] text-white/78 group-hover:text-white/95">
                      {test.name}
                    </span>
                    <span className="mt-1 block truncate font-mono text-[9px] text-white/28">
                      {test.id}
                    </span>
                  </button>
                  {selected && (
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="mr-1 text-cyan-100 hover:bg-cyan-200/8"
                      disabled={running}
                      onClick={onRun}
                      aria-label={`Run ${test.name}`}
                    >
                      {running ? (
                        <LoaderCircle className="animate-spin" />
                      ) : (
                        <Play />
                      )}
                    </Button>
                  )}
                </div>
              )
            })}
          </div>

          <Separator className="my-4" />

          <div className="flex min-h-0 flex-1 flex-col">
            <div className="mb-2 flex shrink-0 items-center justify-between px-1">
              <SectionLabel icon={History}>Past runs</SectionLabel>
              {runs.length > 0 && (
                <span className="font-mono text-[9px] text-white/22">
                  {runs.length.toString().padStart(2, "0")}
                </span>
              )}
            </div>

            {runs.length === 0 ? (
              <div className="mx-1 rounded-md border border-dashed border-white/7 px-3 py-5 text-center">
                <CircleOff className="mx-auto mb-2 size-3.5 text-white/18" />
                <p className="m-0 text-[10px] leading-relaxed text-white/27">
                  Runs from this browser
                  <br />
                  will appear here.
                </p>
              </div>
            ) : (
              <div className="past-runs-scroll min-h-0 flex-1 overflow-y-auto pr-1">
                <div className="space-y-0.5">
                  {runs.map((run) => (
                    <button
                      type="button"
                      key={run.runId}
                      className={cn(
                        "run-row group",
                        selectedRunId === run.runId && "run-row-selected"
                      )}
                      onClick={() => onSelectRun(run)}
                    >
                      <span
                        className={cn(
                          "run-status",
                          run.status === "passed"
                            ? "run-status-pass"
                            : "run-status-fail"
                        )}
                      >
                        {run.status === "passed" ? <Check /> : <X />}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[11px] text-white/64 group-hover:text-white/88">
                          {run.testName}
                        </span>
                        <span className="mt-0.5 block font-mono text-[9px] text-white/25">
                          {formatRunTime(run.createdAt)}
                        </span>
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        <div className="border-t border-white/6 px-3 py-3">
          <div className="flex items-center justify-between">
            <Badge
              variant={source === "live" ? "live" : "neutral"}
              className="normal-case"
            >
              <span
                className={cn(
                  "size-1 rounded-full",
                  source === "live" ? "bg-cyan-300" : "bg-white/30"
                )}
              />
              {source === "connecting"
                ? "connecting"
                : source === "live"
                  ? "tracer live"
                  : "demo data"}
            </Badge>
            <RotateCcw className="size-3 text-white/18" aria-hidden="true" />
          </div>
        </div>
      </div>
    </aside>
  )
}

function SectionLabel({
  icon: Icon,
  children,
}: {
  icon: typeof FlaskConical
  children: string
}) {
  return (
    <div className="flex items-center gap-1.5 font-mono text-[9px] font-medium tracking-[0.14em] text-white/34 uppercase">
      <Icon className="size-3" />
      {children}
    </div>
  )
}

function formatRunTime(timestamp: string): string {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return "unknown"
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    month: "short",
    day: "2-digit",
  }).format(date)
}
