import { useCallback, useEffect, useState } from "react"
import { PanelLeft, Workflow } from "lucide-react"

import { ExecutionHeader } from "@/components/execution-header"
import { MessageGraph } from "@/components/message-graph"
import { StudioSidebar } from "@/components/studio-sidebar"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import { getFixture, getTest, listTests, runTest } from "@/lib/api"
import { expectedGraph, reportGraph } from "@/lib/graph"
import {
  SAMPLE_DEFINITIONS,
  SAMPLE_FIXTURE,
  SAMPLE_GRAPH,
  SAMPLE_TESTS,
} from "@/lib/sample-data"
import type {
  ExpectedMessageNode,
  Fixture,
  MessageGraphNode,
  StoredRun,
  StudioLayout,
  TestDefinitionRevision,
  TestDefinitionSummary,
  TestReport,
} from "@/lib/types"
import { cn } from "@/lib/utils"

const STORED_RUNS_KEY = "rostfrei-tracer-studio-runs-v1"
const MAXIMUM_STORED_RUNS = 16
const STORED_LAYOUT_KEY = "rostfrei-tracer-studio-layout-v1"

function App() {
  const [layout, setLayout] = useState<StudioLayout>(readStoredLayout)
  const [canvasNavigationOpen, setCanvasNavigationOpen] = useState(false)
  const [workbenchNavigationOpen, setWorkbenchNavigationOpen] = useState(
    () => window.matchMedia("(min-width: 1400px)").matches
  )
  const sidebarOpen =
    layout === "canvas" ? canvasNavigationOpen : workbenchNavigationOpen
  const setSidebarOpen =
    layout === "canvas" ? setCanvasNavigationOpen : setWorkbenchNavigationOpen
  const changeLayout = useCallback((next: StudioLayout) => {
    setLayout(next)
    try {
      localStorage.setItem(STORED_LAYOUT_KEY, next)
    } catch {
      // The layout switch remains available when browser storage is disabled.
    }
  }, [])

  useEffect(() => {
    const handleLayoutShortcut = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.isComposing ||
        event.repeat ||
        event.altKey ||
        event.ctrlKey ||
        event.metaKey ||
        event.shiftKey ||
        (event.key !== "1" && event.key !== "2")
      )
        return
      const target = event.composedPath()[0]
      if (
        target instanceof Element &&
        (target.closest(
          "input, textarea, select, [role='textbox'], [role='combobox']"
        ) ||
          (target instanceof HTMLElement && target.isContentEditable))
      )
        return
      event.preventDefault()
      changeLayout(event.key === "1" ? "canvas" : "workbench")
    }
    window.addEventListener("keydown", handleLayoutShortcut)
    return () => window.removeEventListener("keydown", handleLayoutShortcut)
  }, [changeLayout])
  const [source, setSource] = useState<"connecting" | "live" | "demo">(
    "connecting"
  )
  const [tests, setTests] = useState<TestDefinitionSummary[]>(SAMPLE_TESTS)
  const [selectedTestId, setSelectedTestId] = useState<string | undefined>(
    SAMPLE_TESTS[0]?.id
  )
  const [selectedRunId, setSelectedRunId] = useState<string>()
  const [definition, setDefinition] = useState<
    TestDefinitionRevision | undefined
  >(SAMPLE_DEFINITIONS["rent-available-bicycle"])
  const [nodes, setNodes] = useState<MessageGraphNode[]>(() =>
    expectedGraph(
      SAMPLE_DEFINITIONS["rent-available-bicycle"].definition,
      SAMPLE_FIXTURE
    )
  )
  const [runs, setRuns] = useState<StoredRun[]>(readStoredRuns)
  const [running, setRunning] = useState(false)
  const [loading, setLoading] = useState(true)
  const [resultUnavailable, setResultUnavailable] = useState(false)
  const [error, setError] = useState<string>()
  const [storageNote, setStorageNote] = useState<string>()

  useEffect(() => {
    let active = true
    const connect = async () => {
      try {
        const availableTests = await listTests()
        if (!active) return
        const selected =
          availableTests.find((test) => test.id === SAMPLE_TESTS[0]?.id) ??
          availableTests[0]
        if (!selected) {
          setSource("live")
          setTests(availableTests)
          setSelectedTestId(undefined)
          setDefinition(undefined)
          setNodes([])
          return
        }
        const revision = await getTest(selected.id)
        const canonicalFixture = await getFixture(
          revision.definition.setup.fixture
        )
        if (!active) return
        setSource("live")
        setTests(availableTests)
        setSelectedTestId(selected.id)
        setDefinition(revision)
        setNodes(expectedGraph(revision.definition, canonicalFixture))
      } catch {
        if (!active) return
        setSource("demo")
      } finally {
        if (active) setLoading(false)
      }
    }
    void connect()
    return () => {
      active = false
    }
  }, [])

  const selectTest = async (test: TestDefinitionSummary) => {
    if (running || loading) return
    setLoading(true)
    setSelectedTestId(test.id)
    setSelectedRunId(undefined)
    setError(undefined)
    setResultUnavailable(false)
    setDefinition(undefined)
    setNodes([])
    try {
      const { revision, fixture } = await loadTest(test.id)
      setDefinition(revision)
      setNodes(expectedGraph(revision.definition, fixture))
    } catch (selectionError) {
      setError(errorMessage(selectionError))
    } finally {
      setLoading(false)
    }
    if (
      layout === "canvas" ||
      window.matchMedia("(max-width: 1399px)").matches
    ) {
      setSidebarOpen(false)
    }
  }

  const loadTest = async (
    testId: string
  ): Promise<{ revision: TestDefinitionRevision; fixture: Fixture }> => {
    const revision =
      source === "live" ? await getTest(testId) : SAMPLE_DEFINITIONS[testId]
    if (!revision)
      throw new Error("This test definition is no longer available.")
    const fixture =
      source === "live"
        ? await getFixture(revision.definition.setup.fixture)
        : SAMPLE_FIXTURE
    return { revision, fixture }
  }

  const executeSelectedTest = async () => {
    const selected = tests.find((test) => test.id === selectedTestId)
    if (!selected || running || loading || source === "connecting") return
    setRunning(true)
    setError(undefined)
    setResultUnavailable(false)
    setSelectedRunId(undefined)
    setNodes([])

    try {
      // Load the selected test afresh, including when rerunning a historical run.
      const { revision, fixture } = await loadTest(selected.id)
      setDefinition(revision)
      setNodes(
        expectedGraph(revision.definition, fixture)
          .filter((node) => node.context || node.kind === "command")
          .map((node) => ({
            ...node,
            status: node.context ? "idle" : "running",
          }))
      )
      if (source === "live") {
        const report = await runTest(selected.runHref)
        const observedNodes = reportGraph(report, fixture)
        setNodes(observedNodes)
        storeRun(report, selected.name, observedNodes, revision)
      } else {
        const demoNodes = await runDemo(revision, setNodes)
        const report = createDemoReport(revision)
        storeRun(report, selected.name, demoNodes, revision)
      }
    } catch (runError) {
      setError(errorMessage(runError))
      setResultUnavailable(true)
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
    observedNodes: MessageGraphNode[],
    revision: TestDefinitionRevision
  ) => {
    const stored: StoredRun = {
      runId: report.runId,
      testId: report.testId,
      testName,
      status: report.status,

      createdAt: new Date().toISOString(),
      nodes: observedNodes,
      source: source === "live" ? "live" : "demo",
      definition: revision,
      diagnostics: report.comparison.diagnostics,
    }
    const updated = [stored, ...runs].slice(0, MAXIMUM_STORED_RUNS)
    setRuns(updated)
    try {
      localStorage.setItem(STORED_RUNS_KEY, JSON.stringify(updated))
      setStorageNote(undefined)
    } catch {
      setStorageNote(
        "This run is available for this session. Browser storage could not save it."
      )
    }
    setSelectedRunId(stored.runId)
  }

  const selectRun = (run: StoredRun) => {
    if (running || loading) return
    setSelectedRunId(run.runId)
    setSelectedTestId(run.testId)
    setNodes(run.nodes)
    setDefinition(run.definition)
    setResultUnavailable(false)
    setError(undefined)
    if (
      layout === "canvas" ||
      window.matchMedia("(max-width: 1399px)").matches
    ) {
      setSidebarOpen(false)
    }
  }

  const selectedRun = runs.find((run) => run.runId === selectedRunId)
  const selectedTest = tests.find((test) => test.id === selectedTestId)
  const view = running
    ? "running"
    : resultUnavailable
      ? "unavailable"
      : selectedRun
        ? "observed"
        : "expected"
  const displayedSource = selectedRun
    ? (selectedRun.source ?? "stored")
    : source

  return (
    <TooltipProvider>
      <div
        className={cn(
          "studio-shell",
          `studio-layout-${layout}`,
          sidebarOpen && "sidebar-is-open"
        )}
      >
        <header className="studio-topbar">
          <Button
            variant="ghost"
            size="icon"
            className="sidebar-toggle"
            onClick={() => setSidebarOpen((open) => !open)}
            aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
            aria-expanded={sidebarOpen}
            aria-controls="studio-navigation"
          >
            <PanelLeft />
          </Button>
          <div className="studio-logo" aria-label="Rostfrei Tracer Studio">
            <Workflow size={19} />
            <span>
              rostfrei <strong>Tracer Studio</strong>
            </span>
          </div>
          <div
            className="layout-switch"
            role="group"
            aria-label="Studio layout"
          >
            <button
              type="button"
              data-layout="canvas"
              aria-keyshortcuts="1"
              title="Canvas (1)"
              aria-pressed={layout === "canvas"}
              onClick={() => changeLayout("canvas")}
            >
              Canvas
              <kbd aria-hidden="true">1</kbd>
            </button>
            <button
              type="button"
              data-layout="workbench"
              aria-keyshortcuts="2"
              title="Workbench (2)"
              aria-pressed={layout === "workbench"}
              onClick={() => changeLayout("workbench")}
            >
              Workbench
              <kbd aria-hidden="true">2</kbd>
            </button>
          </div>
          <div className="connection-status" data-source={displayedSource}>
            <span className="connection-dot" />
            {displayedSource === "connecting"
              ? "Connecting"
              : displayedSource === "live"
                ? "Isolated Test"
                : displayedSource === "stored"
                  ? "Saved run"
                  : "Demo data"}
            {displayedSource === "demo" && (
              <span className="connection-detail">Simulated execution</span>
            )}
          </div>
        </header>

        <StudioSidebar
          open={sidebarOpen}
          tests={tests}
          selectedTestId={selectedTestId}
          selectedRunId={selectedRunId}
          runs={runs}
          busy={running || loading}
          storageNote={storageNote}
          onClose={() => setSidebarOpen(false)}
          onSelectTest={(test) => void selectTest(test)}
          onSelectRun={selectRun}
        />

        {sidebarOpen && (
          <button
            type="button"
            className="sidebar-scrim"
            onClick={() => setSidebarOpen(false)}
            aria-label="Close sidebar"
          />
        )}

        <main className="studio-main">
          <ExecutionHeader
            layout={layout}
            name={selectedRun?.testName ?? selectedTest?.name}
            definition={definition}
            run={selectedRun}
            nodes={nodes}
            view={view}
            loading={loading}
            canRun={
              Boolean(selectedTest) &&
              !loading &&
              !running &&
              (Boolean(definition) || Boolean(selectedRun))
            }
            demo={source === "demo"}
            error={error}
            onRun={() => void executeSelectedTest()}
          />
          <MessageGraph
            key={selectedRunId ?? selectedTestId ?? "empty"}
            layoutMode={layout}
            nodes={nodes}
            view={view}
          />
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
      : expectedGraph(definition.definition, SAMPLE_FIXTURE)
  const expectedRoot = subjectCommand(definition)
  const rootStatus: MessageGraphNode["status"] =
    expectedRoot?.outcome === "accepted" ? "accepted" : "rejected"
  const demoResponse = createDemoCommandResponse(definition)
  const subjectIndex = preview.findIndex(
    (node) => node.kind === "command" && !node.context
  )
  const completed: MessageGraphNode[] = preview.map((node, index) => ({
    ...node,
    status: index === subjectIndex ? rootStatus : "accepted",
    response:
      index === subjectIndex ? (node.response ?? demoResponse) : node.response,
  }))
  render(
    completed.slice(0, subjectIndex + 1).map((node, index) => ({
      ...node,
      status: index === subjectIndex ? "running" : node.status,
      response: index === subjectIndex ? undefined : node.response,
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

function createDemoCommandResponse(
  definition: TestDefinitionRevision
): unknown {
  const root = subjectCommand(definition)
  const outcome = root?.outcome
  if (!outcome || outcome === "accepted") {
    return { status: "accepted", value: null }
  }

  const message =
    outcome.rejected.code === "BICYCLE_UNAVAILABLE"
      ? "The requested bicycle cannot currently be rented."
      : "The command was rejected."
  const payload = outcome.rejected.payload
  const details =
    typeof payload === "object" && payload !== null && !Array.isArray(payload)
      ? payload
      : payload === undefined
        ? {}
        : { payload }
  return {
    status: "rejected",
    value: {
      classification: "conflict",
      code: outcome.rejected.code,
      details,
      message,
    },
  }
}

function createDemoReport(definition: TestDefinitionRevision): TestReport {
  const identity = crypto.randomUUID()
  const operationId = `demo-operation-${identity}`
  const correlationId = `demo-correlation-${identity}`
  const expectedRoot = subjectCommand(definition)
  const accepted = expectedRoot?.outcome === "accepted"
  const commandOutcome = {
    responseMessageId: `demo-response-${identity}`,
    commandMessageId: `demo-command-${identity}`,
    correlationId,
    observationOrder: 2,
    outcome: accepted
      ? ({ status: "accepted", value: null } as const)
      : ({
          status: "rejected",
          value: {
            classification: "conflict",
            code:
              expectedRoot?.kind === "command" &&
              expectedRoot.outcome !== "accepted"
                ? expectedRoot.outcome.rejected.code
                : "COMMAND_REJECTED",
            message: "The command was rejected.",
          },
        } as const),
  }
  const command =
    expectedRoot?.kind === "command"
      ? expectedRoot
      : {
          name: "unknown-command",
          schemaVersion: 1,
          context: "unknown",
          payload: undefined,
        }
  return {
    runId: `demo-${identity}`,
    testId: definition.definition.id,
    revision: definition.revision,
    status: "passed",
    expected: definition.definition.expected,
    observed: {
      messages: [
        {
          kind: "command",
          messageId: commandOutcome.commandMessageId,
          correlationId,
          observationOrder: 1,
          name: command.name,
          schemaVersion: command.schemaVersion,
          context: command.context,
          payload: command.payload,
        },
      ],
      commandOutcomes: [commandOutcome],
    },
    comparison: {
      status: "passed",
      matches: expectedRoot
        ? [
            {
              expectedKey: expectedRoot.key,
              observedMessageId: commandOutcome.commandMessageId,
            },
          ]
        : [],
      diagnostics: [],
    },
    commandOutcome,
    operationId,
    correlationId,
    operationHref: `/operations/${operationId}`,
    operationEventsHref: `/operations/${operationId}/events`,
    correlationEventsHref: `/correlations/${correlationId}/events`,
    operation: {
      operationId,
      correlationId,
      operationEventsHref: `/operations/${operationId}/events`,
      correlationEventsHref: `/correlations/${correlationId}/events`,
      messageSeriesHref: `/operations/${operationId}/message-series`,
      events: {
        kind: "observed",
        href: `/correlations/${correlationId}/events`,
      },
      mode: "test",
      status: "completed",
      context: command.context,
      command: command.name,
      schemaVersion: command.schemaVersion,
      latestEventId: 2,
      result: { decision: accepted ? "accepted" : "rejected" },
    },
  }
}

type ExpectedCommandNode = Extract<ExpectedMessageNode, { kind: "command" }>

function subjectCommand(
  revision: TestDefinitionRevision
): ExpectedCommandNode | undefined {
  return revision.definition.expected.graphs[0]?.nodes.find(
    (node): node is ExpectedCommandNode =>
      node.kind === "command" && !node.parentKey
  )
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds))
}

function readStoredRuns(): StoredRun[] {
  try {
    const value = localStorage.getItem(STORED_RUNS_KEY)
    const parsed: unknown = value ? JSON.parse(value) : []
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter(
        (run): run is StoredRun =>
          typeof run?.runId === "string" &&
          typeof run.testId === "string" &&
          typeof run.testName === "string" &&
          (run.status === "passed" || run.status === "failed") &&
          typeof run.createdAt === "string" &&
          Array.isArray(run.nodes) &&
          run.nodes.every(
            (node: MessageGraphNode) =>
              node &&
              typeof node.id === "string" &&
              typeof node.name === "string" &&
              ["command", "domain-event", "integration-event"].includes(
                node.kind
              )
          )
      )
      .slice(0, MAXIMUM_STORED_RUNS)
  } catch {
    return []
  }
}

function readStoredLayout(): StudioLayout {
  try {
    return localStorage.getItem(STORED_LAYOUT_KEY) === "workbench"
      ? "workbench"
      : "canvas"
  } catch {
    return "canvas"
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "The Tracer request failed"
}

export default App
