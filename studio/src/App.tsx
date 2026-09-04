import { useEffect, useState } from "react"
import {
  AlertTriangle,
  ChevronLeft,
  ChevronRight,
  LoaderCircle,
} from "lucide-react"

import { MessageGraph } from "@/components/message-graph"
import { StudioSidebar } from "@/components/studio-sidebar"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import { collectCorrelation, getTest, listTests, runTest } from "@/lib/api"
import { correlationGraph, expectedGraph } from "@/lib/graph"
import {
  SAMPLE_DEFINITIONS,
  SAMPLE_GRAPH,
  SAMPLE_TESTS,
} from "@/lib/sample-data"
import type {
  MessageGraphNode,
  StoredRun,
  TestDefinitionRevision,
  TestDefinitionSummary,
  TestReport,
} from "@/lib/types"
import { cn } from "@/lib/utils"

const STORED_RUNS_KEY = "rostfrei-tracer-studio-runs-v1"
const MAXIMUM_STORED_RUNS = 16

function App() {
  const [sidebarOpen, setSidebarOpen] = useState(
    () => window.matchMedia("(min-width: 768px)").matches
  )
  const [source, setSource] = useState<"connecting" | "live" | "demo">(
    "connecting"
  )
  const [tests, setTests] = useState<TestDefinitionSummary[]>(SAMPLE_TESTS)
  const [selectedTestId, setSelectedTestId] = useState<string | undefined>(
    SAMPLE_TESTS[0]?.id
  )
  const [selectedRunId, setSelectedRunId] = useState<string>()
  const [definition, setDefinition] = useState<TestDefinitionRevision>(
    SAMPLE_DEFINITIONS["rent-available-bicycle"]
  )
  const [nodes, setNodes] = useState<MessageGraphNode[]>(SAMPLE_GRAPH)
  const [runs, setRuns] = useState<StoredRun[]>(readStoredRuns)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState<string>()

  useEffect(() => {
    let active = true
    const connect = async () => {
      try {
        const availableTests = await listTests()
        if (!active) return
        setSource("live")
        setTests(availableTests)
        const selected =
          availableTests.find((test) => test.id === SAMPLE_TESTS[0]?.id) ??
          availableTests[0]
        if (!selected) {
          setSelectedTestId(undefined)
          setNodes([])
          return
        }
        const revision = await getTest(selected.id)
        if (!active) return
        setSelectedTestId(selected.id)
        setDefinition(revision)
        setNodes(expectedGraph(revision.definition))
      } catch {
        if (!active) return
        setSource("demo")
      }
    }
    void connect()
    return () => {
      active = false
    }
  }, [])

  const selectTest = async (test: TestDefinitionSummary) => {
    setSelectedTestId(test.id)
    setSelectedRunId(undefined)
    setError(undefined)
    if (source === "live") {
      try {
        const revision = await getTest(test.id)
        setDefinition(revision)
        setNodes(expectedGraph(revision.definition))
      } catch (selectionError) {
        setError(errorMessage(selectionError))
      }
    } else {
      const revision = SAMPLE_DEFINITIONS[test.id]
      if (revision) {
        setDefinition(revision)
        setNodes(expectedGraph(revision.definition))
      }
    }
    if (window.matchMedia("(max-width: 767px)").matches) {
      setSidebarOpen(false)
    }
  }

  const executeSelectedTest = async () => {
    const selected = tests.find((test) => test.id === selectedTestId)
    if (!selected || running) return
    setRunning(true)
    setError(undefined)
    setSelectedRunId(undefined)
    setNodes((current) =>
      current.map((node) => ({
        ...node,
        status: node.context
          ? node.status
          : node.kind === "command"
            ? "running"
            : "idle",
      }))
    )

    try {
      if (source === "live") {
        const report = await runTest(selected.runHref)
        setNodes([])
        const events = await collectCorrelation(
          report.correlationId,
          async (observedEvents) => {
            setNodes(
              correlationGraph(observedEvents, definition.definition, report)
            )
            await delay(880)
          }
        )
        const observedNodes = correlationGraph(
          events,
          definition.definition,
          report
        )
        setNodes(observedNodes)
        storeRun(report, selected.name, observedNodes)
      } else {
        const demoNodes = await runDemo(definition, setNodes)
        const report = createDemoReport(definition)
        storeRun(report, selected.name, demoNodes)
      }
    } catch (runError) {
      setError(errorMessage(runError))
      setNodes((current) =>
        current.map((node) => ({
          ...node,
          status:
            !node.context && node.kind === "command" ? "failed" : node.status,
        }))
      )
    } finally {
      setRunning(false)
    }
  }

  const storeRun = (
    report: TestReport,
    testName: string,
    observedNodes: MessageGraphNode[]
  ) => {
    const stored: StoredRun = {
      runId: report.runId,
      testId: report.testId,
      testName,
      status: report.status,
      outcome: report.outcome,
      createdAt: new Date().toISOString(),
      nodes: observedNodes,
    }
    setRuns((current) => {
      const updated = [stored, ...current].slice(0, MAXIMUM_STORED_RUNS)
      localStorage.setItem(STORED_RUNS_KEY, JSON.stringify(updated))
      return updated
    })
    setSelectedRunId(stored.runId)
  }

  const selectRun = (run: StoredRun) => {
    setSelectedRunId(run.runId)
    setSelectedTestId(run.testId)
    setNodes(run.nodes)
    setError(undefined)
    if (window.matchMedia("(max-width: 767px)").matches) {
      setSidebarOpen(false)
    }
  }

  const currentLabel = selectedRunId
    ? runs.find((run) => run.runId === selectedRunId)?.testName
    : definition.definition.name

  return (
    <TooltipProvider>
      <div className="studio-shell">
        <header className="studio-topbar">
          <div className="studio-logo" aria-label="Rostfrei Tracer Studio">
            <span>rostfrei</span>
            <strong>TRACER STUDIO</strong>
          </div>

          <Button
            variant="ghost"
            size="icon-sm"
            className={cn(
              "sidebar-toggle",
              sidebarOpen && "sidebar-toggle-open"
            )}
            onClick={() => setSidebarOpen((open) => !open)}
            aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
            aria-expanded={sidebarOpen}
          >
            {sidebarOpen ? <ChevronLeft /> : <ChevronRight />}
          </Button>

          <div className="ml-auto flex min-w-0 items-center gap-2">
            {running && (
              <Badge variant="live">
                <LoaderCircle className="size-2.5 animate-spin" />
                observing
              </Badge>
            )}
            <span className="hidden max-w-[34vw] truncate font-mono text-[9px] text-white/24 sm:block">
              {currentLabel}
            </span>
          </div>
        </header>

        <StudioSidebar
          open={sidebarOpen}
          tests={tests}
          selectedTestId={selectedTestId}
          selectedRunId={selectedRunId}
          runs={runs}
          source={source}
          running={running}
          onClose={() => setSidebarOpen(false)}
          onSelectTest={(test) => void selectTest(test)}
          onSelectRun={selectRun}
          onRun={() => void executeSelectedTest()}
        />

        {sidebarOpen && (
          <button
            type="button"
            className="sidebar-scrim"
            onClick={() => setSidebarOpen(false)}
            aria-label="Close sidebar"
          />
        )}

        <main
          className={cn("studio-main", sidebarOpen && "studio-main-shifted")}
        >
          <div className="graph-atmosphere" />
          <MessageGraph nodes={nodes} running={running} />

          <div className="graph-caption" aria-live="polite">
            <span className="graph-caption-dot" />
            <span>
              {nodes.filter((node) => node.context !== "fixture").length}{" "}
              messages
            </span>
            <span className="text-white/13">/</span>
            <span>
              {nodes.some(
                (node) => !node.context && node.edgeFidelity === "grouped"
              )
                ? "grouped where causation is absent"
                : "exact causality"}
            </span>
          </div>

          {error && (
            <div className="studio-error" role="alert">
              <AlertTriangle className="size-3.5 shrink-0 text-amber-300" />
              <span className="min-w-0 flex-1 truncate">{error}</span>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setError(undefined)}
                aria-label="Dismiss error"
              >
                <ChevronRight />
              </Button>
            </div>
          )}
        </main>
      </div>
    </TooltipProvider>
  )
}

async function runDemo(
  definition: TestDefinitionRevision,
  render: (nodes: MessageGraphNode[]) => void
): Promise<MessageGraphNode[]> {
  const preview =
    definition.definition.id === "rent-available-bicycle"
      ? SAMPLE_GRAPH
      : expectedGraph(definition.definition)
  const rootStatus: MessageGraphNode["status"] =
    definition.definition.then.outcome === "accepted" ? "accepted" : "rejected"
  const subjectIndex = preview.findIndex(
    (node) => node.kind === "command" && !node.context
  )
  const completed: MessageGraphNode[] = preview.map((node, index) => ({
    ...node,
    status: index === subjectIndex ? rootStatus : "accepted",
    response:
      index === subjectIndex
        ? (node.response ??
          (definition.definition.then.outcome === "accepted"
            ? { decision: "accepted" }
            : {
                decision: "rejected",
                rejection: definition.definition.then.outcome.rejected,
              }))
        : node.response,
  }))
  render(
    completed.slice(0, subjectIndex + 1).map((node, index) => ({
      ...node,
      status: index === subjectIndex ? "running" : node.status,
    }))
  )
  for (let index = subjectIndex + 1; index < completed.length; index += 1) {
    await delay(880)
    render(completed.slice(0, index + 1))
  }
  await delay(180)
  render(completed)
  return completed
}

function createDemoReport(definition: TestDefinitionRevision): TestReport {
  const identity = crypto.randomUUID()
  const accepted = definition.definition.then.outcome === "accepted"
  return {
    runId: `demo-${identity}`,
    testId: definition.definition.id,
    revision: definition.revision,
    status: "passed",
    operationId: `demo-operation-${identity}`,
    correlationId: `demo-correlation-${identity}`,
    outcome: accepted ? "accepted" : "rejected",
  }
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds))
}

function readStoredRuns(): StoredRun[] {
  try {
    const value = localStorage.getItem(STORED_RUNS_KEY)
    return value ? (JSON.parse(value) as StoredRun[]) : []
  } catch {
    return []
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "The Tracer request failed"
}

export default App
