import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react"
import {
  Check,
  CircleOff,
  FlaskConical,
  GripHorizontal,
  History,
  LoaderCircle,
  Play,
  RotateCcw,
  X,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import type { StoredRun, TestDefinitionSummary } from "@/lib/types"
import { cn } from "@/lib/utils"

interface StudioSidebarProps {
  testsOpen: boolean
  runsOpen: boolean
  tests: TestDefinitionSummary[]
  selectedTestId?: string
  selectedRunId?: string
  runs: StoredRun[]
  source: "connecting" | "live" | "demo"
  running: boolean
  onCloseTests: () => void
  onCloseRuns: () => void
  onSelectTest: (test: TestDefinitionSummary) => void
  onSelectRun: (run: StoredRun) => void
  onRun: () => void
}

let nextPanelLayer = 60

export function StudioSidebar({
  testsOpen,
  runsOpen,
  tests,
  selectedTestId,
  selectedRunId,
  runs,
  source,
  running,
  onCloseTests,
  onCloseRuns,
  onSelectTest,
  onSelectRun,
  onRun,
}: StudioSidebarProps) {
  return (
    <>
      <DraggablePanel
        id="studio-tests-panel"
        className="studio-tests-panel"
        label="Tests"
        icon={FlaskConical}
        open={testsOpen}
        initialPosition={{ x: 16, y: 70 }}
        closeLabel="Close tests panel"
        onClose={onCloseTests}
      >
        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          <div className="space-y-0.5">
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
                    <span className="block truncate text-[14px] text-white/78 group-hover:text-white/95">
                      {test.name}
                    </span>
                    <span className="mt-1 block truncate font-mono text-[12px] text-white/52">
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
      </DraggablePanel>

      <DraggablePanel
        id="studio-runs-panel"
        className="studio-runs-panel"
        label="Past runs"
        icon={History}
        count={runs.length}
        open={runsOpen}
        initialPosition={{ x: 312, y: 70 }}
        closeLabel="Close runs panel"
        onClose={onCloseRuns}
      >
        <div className="flex min-h-0 flex-1 flex-col px-2 pb-3">
          {runs.length === 0 ? (
            <div className="mx-1 rounded-md border border-dashed border-white/7 px-3 py-5 text-center">
              <CircleOff className="mx-auto mb-2 size-3.5 text-white/18" />
              <p className="m-0 text-[13px] leading-relaxed text-white/58">
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
                      <span className="block truncate text-[14px] text-white/78 group-hover:text-white/95">
                        {run.testName}
                      </span>
                      <span className="mt-0.5 block font-mono text-[12px] text-white/52">
                        {formatRunTime(run.createdAt)}
                      </span>
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
      </DraggablePanel>
    </>
  )
}

interface Point {
  x: number
  y: number
}

interface DraggablePanelProps {
  id: string
  className: string
  label: string
  icon: typeof FlaskConical
  count?: number
  open: boolean
  initialPosition: Point
  closeLabel: string
  onClose: () => void
  children: ReactNode
}

export function DraggablePanel({
  id,
  className,
  label,
  icon: Icon,
  count,
  open,
  initialPosition,
  closeLabel,
  onClose,
  children,
}: DraggablePanelProps) {
  const panelRef = useRef<HTMLElement>(null)
  const drag = useRef<{
    pointerId: number
    offsetX: number
    offsetY: number
    position: Point
  } | null>(null)
  const [position, setPosition] = useState(initialPosition)

  useEffect(() => {
    const panel = panelRef.current
    const keepOnScreen = () =>
      setPosition((current) => clampPanelPosition(panel, current))
    keepOnScreen()
    window.addEventListener("resize", keepOnScreen)
    return () => {
      window.removeEventListener("resize", keepOnScreen)
      panel?.classList.remove("studio-sidebar-dragging")
      document.body.classList.remove("is-dragging-panel")
    }
  }, [])

  useEffect(() => {
    if (open && panelRef.current) {
      panelRef.current.style.zIndex = String(++nextPanelLayer)
    }
  }, [open])

  const finishDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return
    const finalPosition = drag.current.position
    drag.current = null
    setPosition(finalPosition)
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    panelRef.current?.classList.remove("studio-sidebar-dragging")
    document.body.classList.remove("is-dragging-panel")
  }

  const style = {
    left: position.x,
    top: position.y,
  } satisfies CSSProperties

  return (
    <aside
      ref={panelRef}
      id={id}
      className={cn("studio-sidebar", className, open && "studio-sidebar-open")}
      style={style}
      aria-label={label}
      aria-hidden={!open}
      inert={!open}
      onPointerDownCapture={() => {
        if (panelRef.current) {
          panelRef.current.style.zIndex = String(++nextPanelLayer)
        }
      }}
    >
      <div className="flex h-full min-h-0 flex-col">
        <div
          className="studio-panel-header"
          onPointerDown={(event) => {
            if (
              event.button !== 0 ||
              (event.target instanceof Element &&
                event.target.closest("button"))
            ) {
              return
            }
            const bounds = panelRef.current?.getBoundingClientRect()
            if (!bounds) return
            drag.current = {
              pointerId: event.pointerId,
              offsetX: event.clientX - bounds.left,
              offsetY: event.clientY - bounds.top,
              position,
            }
            event.currentTarget.setPointerCapture(event.pointerId)
            panelRef.current?.classList.add("studio-sidebar-dragging")
            document.body.classList.add("is-dragging-panel")
          }}
          onPointerMove={(event) => {
            if (drag.current?.pointerId !== event.pointerId) return
            const nextPosition = clampPanelPosition(panelRef.current, {
              x: event.clientX - drag.current.offsetX,
              y: event.clientY - drag.current.offsetY,
            })
            drag.current.position = nextPosition
            if (panelRef.current) {
              panelRef.current.style.left = `${nextPosition.x}px`
              panelRef.current.style.top = `${nextPosition.y}px`
            }
          }}
          onPointerUp={finishDrag}
          onPointerCancel={finishDrag}
        >
          <div className="flex items-center gap-2">
            <SectionLabel icon={Icon}>{label}</SectionLabel>
            {count !== undefined && count > 0 && (
              <span className="font-mono text-[12px] text-white/50">
                {count.toString().padStart(2, "0")}
              </span>
            )}
          </div>
          <GripHorizontal
            className="studio-panel-grip size-3.5"
            aria-hidden="true"
          />
          <Button
            variant="ghost"
            size="icon-sm"
            className="sidebar-close"
            onClick={onClose}
            aria-label={closeLabel}
          >
            <X />
          </Button>
        </div>
        {children}
      </div>
    </aside>
  )
}

function clampPanelPosition(panel: HTMLElement | null, next: Point): Point {
  if (!panel) return next
  const bounds = panel.getBoundingClientRect()
  const margin = 8
  const topInset = 62
  const bottomInset = 64
  return {
    x: Math.min(
      Math.max(margin, next.x),
      Math.max(margin, window.innerWidth - bounds.width - margin)
    ),
    y: Math.min(
      Math.max(topInset, next.y),
      Math.max(topInset, window.innerHeight - bounds.height - bottomInset)
    ),
  }
}

function SectionLabel({
  icon: Icon,
  children,
}: {
  icon: typeof FlaskConical
  children: string
}) {
  return (
    <div className="flex items-center gap-1.5 font-mono text-[12px] font-medium tracking-[0.14em] text-white/62 uppercase">
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
