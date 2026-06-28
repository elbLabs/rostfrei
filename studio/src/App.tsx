import { useCallback, useEffect, useState } from "react"
import { PanelLeft, Send, Workflow } from "lucide-react"

import { ConnectionNotice } from "@/components/connection-notice"
import {
  CommandPanel,
  type CommandExecutionRequest,
} from "@/components/command-panel"
import {
  CommandExecutionHeader,
  type CommandView,
} from "@/components/command-execution-header"
import { ExecutionHeader } from "@/components/execution-header"
import { MessageGraph } from "@/components/message-graph"
import { StudioSidebar } from "@/components/studio-sidebar"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import {
  CommandSubmissionIndeterminateError,
  getCatalog,
  getFixture,
  getTest,
  listTests,
  runTest,
  submitCommand,
} from "@/lib/api"
import { expectedGraph, operationGraph, reportGraph } from "@/lib/graph"
import type {
  Fixture,
  CommandExecutionResult,
  MessageGraphNode,
  StoredRun,
  StudioLayout,
  TestDefinitionRevision,
  TestDefinitionSummary,
  TestReport,
  TracerCatalog,
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

  const [source, setSource] = useState<"connecting" | "live" | "disconnected">(
    "connecting"
  )
  const [connectionAttempt, setConnectionAttempt] = useState(0)
  const [connectionError, setConnectionError] = useState<string>()
  const [testDiscoveryError, setTestDiscoveryError] = useState<string>()
  const [catalog, setCatalog] = useState<TracerCatalog>()
  const [commandOpen, setCommandOpen] = useState(false)
  const [commandResult, setCommandResult] = useState<CommandExecutionResult>()
  const [commandError, setCommandError] = useState<string>()
  const [activeCommand, setActiveCommand] = useState<CommandView>()
  const [testStateRevision, setTestStateRevision] = useState(0)
  const [tests, setTests] = useState<TestDefinitionSummary[]>([])
  const [selectedTestId, setSelectedTestId] = useState<string>()
  const [selectedRunId, setSelectedRunId] = useState<string>()
  const [definition, setDefinition] = useState<TestDefinitionRevision>()
  const [nodes, setNodes] = useState<MessageGraphNode[]>([])
  const [runs, setRuns] = useState<StoredRun[]>(readStoredRuns)
  const [running, setRunning] = useState(false)
  const [loading, setLoading] = useState(true)
  const [resultUnavailable, setResultUnavailable] = useState(false)
  const [error, setError] = useState<string>()
  const [storageNote, setStorageNote] = useState<string>()

  useEffect(() => {
    const toggleCommand = (event: KeyboardEvent) => {
      if (
        (event.ctrlKey || event.metaKey) &&
        event.shiftKey &&
        !event.altKey &&
        event.code === "KeyK" &&
        catalog
      ) {
        event.preventDefault()
        setCommandOpen((open) => !open)
      }
    }
    window.addEventListener("keydown", toggleCommand)
    return () => window.removeEventListener("keydown", toggleCommand)
  }, [catalog])

  useEffect(() => {
    let active = true
    const connect = async () => {
      let availableCatalog: TracerCatalog
      try {
        availableCatalog = await getCatalog()
      } catch (connectError) {
        if (active) {
          setSource("disconnected")
          setConnectionError(errorMessage(connectError))
          setLoading(false)
        }
        return
      }
      if (!active) return
      setCatalog(availableCatalog)
      setSource("live")
      const definitionsHref =
        availableCatalog.testRepository?.definitionsHref ??
        availableCatalog.behavioralTest?.definitionsHref
      if (!definitionsHref) {
        setLoading(false)
        return
      }
      try {
        const availableTests = await listTests(definitionsHref)
        if (!active) return
        const selected = availableTests[0]
        if (!selected) {
          setSource("live")
          setTests([])
          return
        }
        const revision = await getTest(selected.definitionHref)
        const fixture = await getFixture(revision.definition.setup.fixture)
        const expectedNodes = expectedGraph(revision.definition, fixture)
        if (!active) return
        setTests(availableTests)
        setSelectedTestId(selected.id)
        setDefinition(revision)
        setNodes(expectedNodes)
        setSource("live")
      } catch (testError) {
        if (!active) return
        setTestDiscoveryError(errorMessage(testError))
      } finally {
        if (active) setLoading(false)
      }
    }
    void connect()
    return () => {
      active = false
    }
  }, [connectionAttempt])

  const retryConnection = () => {
    if (loading || running) return
    setSource("connecting")
    setLoading(true)
    setConnectionError(undefined)
    setTestDiscoveryError(undefined)
    setCatalog(undefined)
    setActiveCommand(undefined)
    setCommandResult(undefined)
    setCommandError(undefined)
    setError(undefined)
    setResultUnavailable(false)
    setTests([])
    setSelectedTestId(undefined)
    setSelectedRunId(undefined)
    setDefinition(undefined)
    setNodes([])
    setConnectionAttempt((attempt) => attempt + 1)
  }

  const loadTest = async (
    test: TestDefinitionSummary
  ): Promise<{ revision: TestDefinitionRevision; fixture: Fixture }> => {
    const revision = await getTest(test.definitionHref)
    const fixture = await getFixture(revision.definition.setup.fixture)
    return { revision, fixture }
  }

  const selectTest = async (test: TestDefinitionSummary) => {
    if (running || loading || source !== "live") return
    setLoading(true)
    setActiveCommand(undefined)
    setCommandResult(undefined)
    setCommandError(undefined)
    setSelectedTestId(test.id)
    setSelectedRunId(undefined)
    setError(undefined)
    setResultUnavailable(false)
    setDefinition(undefined)
    setNodes([])
    try {
      const { revision, fixture } = await loadTest(test)
      setDefinition(revision)
      setNodes(expectedGraph(revision.definition, fixture))
    } catch (selectionError) {
      setError(errorMessage(selectionError))
    } finally {
      setLoading(false)
    }
    if (layout === "canvas" || window.matchMedia("(max-width: 1399px)").matches)
      setSidebarOpen(false)
  }

  const executeSelectedTest = async () => {
    const selected = tests.find((test) => test.id === selectedTestId)
    if (!selected || running || loading || source !== "live") return
    setRunning(true)
    setActiveCommand(undefined)
    setCommandResult(undefined)
    setCommandError(undefined)
    setError(undefined)
    setResultUnavailable(false)
    setSelectedRunId(undefined)
    setNodes([])
    try {
      // Reruns load the selected test's current definition and fixture.
      const { revision, fixture } = await loadTest(selected)
      setDefinition(revision)
      setNodes(
        expectedGraph(revision.definition, fixture)
          .filter((node) => node.context || node.kind === "command")
          .map((node) => ({
            ...node,
            status: node.context ? "idle" : "running",
          }))
      )
      const report = await runTest(selected.runHref)
      const observedNodes = reportGraph(report, fixture)
      setNodes(observedNodes)
      storeRun(report, selected.name, observedNodes, revision)
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
      setTestStateRevision((current) => current + 1)
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
      source: "live",
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
    setActiveCommand(undefined)
    setCommandResult(undefined)
    setCommandError(undefined)
    setSelectedTestId(run.testId)
    setNodes(run.nodes)
    setDefinition(run.definition)
    setResultUnavailable(false)
    setError(undefined)
    if (layout === "canvas" || window.matchMedia("(max-width: 1399px)").matches)
      setSidebarOpen(false)
  }

  const executeCommand = async (
    request: CommandExecutionRequest
  ): Promise<boolean> => {
    if (running || loading || source !== "live") return false
    setRunning(true)
    setError(undefined)
    setCommandError(undefined)
    setCommandResult(undefined)
    setSelectedRunId(undefined)
    setSelectedTestId(undefined)
    setResultUnavailable(false)
    setActiveCommand({ request })
    const pending: MessageGraphNode = {
      id: `${request.mode}-${crypto.randomUUID()}`,
      kind: "command",
      subject: true,
      name: request.commandId,
      schemaVersion: request.schemaVersion,
      boundedContext: request.context,
      payload: request.payload,
      status: "running",
    }
    setNodes([pending])
    let rotateKey = false
    try {
      const result = await submitCommand(
        request.mode,
        request.submitHrefTemplate,
        request.schemaVersion,
        request.payloadJson,
        request.mode === "test" ? request.idempotencyKey : undefined
      )
      rotateKey =
        result.operation.status === "completed" ||
        result.operation.status === "failed"
      setCommandResult(result)
      setActiveCommand({ request, result })
      setNodes(
        result.series
          ? operationGraph(result.operation, result.series)
          : [
              {
                ...pending,
                response: result.operation.result ?? result.operation.failure,
                status: operationNodeStatus(result.operation),
              },
            ]
      )
    } catch (commandFailure) {
      const message = errorMessage(commandFailure)
      setCommandError(message)
      setActiveCommand({ request, error: message })
      setNodes([
        {
          ...pending,
          response: { message },
          status:
            commandFailure instanceof CommandSubmissionIndeterminateError
              ? "indeterminate"
              : "failed",
        },
      ])
    } finally {
      if (request.mode === "test" && rotateKey)
        setTestStateRevision((current) => current + 1)
      setRunning(false)
    }
    return rotateKey
  }

  const selectedRun = runs.find((run) => run.runId === selectedRunId)
  const selectedTest = tests.find((test) => test.id === selectedTestId)
  const view = running
    ? "running"
    : resultUnavailable || activeCommand?.error
      ? "unavailable"
      : activeCommand
        ? activeCommand.request.mode === "preview"
          ? "predicted"
          : "observed"
        : selectedRun
          ? "observed"
          : "expected"

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
          <Button
            className="command-panel-trigger"
            variant="ghost"
            size="sm"
            disabled={!catalog}
            onClick={() => setCommandOpen((open) => !open)}
            aria-controls="studio-command-panel"
            aria-expanded={commandOpen}
            aria-label={
              commandOpen ? "Close command panel" : "Open command panel"
            }
            aria-keyshortcuts="Meta+Shift+K Control+Shift+K"
            title="Command (Ctrl/⌘ Shift K)"
          >
            <Send />
            <span>Command</span>
          </Button>
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
              Canvas<kbd aria-hidden="true">1</kbd>
            </button>
            <button
              type="button"
              data-layout="workbench"
              aria-keyshortcuts="2"
              title="Workbench (2)"
              aria-pressed={layout === "workbench"}
              onClick={() => changeLayout("workbench")}
            >
              Workbench<kbd aria-hidden="true">2</kbd>
            </button>
          </div>
          <div
            className="connection-status"
            data-source={source}
            data-tracer-source={source}
          >
            <span className="connection-dot" />
            {source === "connecting"
              ? "Connecting"
              : source === "live"
                ? "Isolated Test"
                : "Disconnected"}
            {selectedRun && (
              <span className="connection-detail">Saved run</span>
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
        <CommandPanel
          open={commandOpen}
          catalog={catalog}
          busy={running || loading}
          result={commandResult}
          requestError={commandError}
          testStateRevision={testStateRevision}
          onClose={() => setCommandOpen(false)}
          onExecute={executeCommand}
          onEdit={() => {
            setCommandResult(undefined)
            setCommandError(undefined)
          }}
          onRefreshTestState={() =>
            setTestStateRevision((current) => current + 1)
          }
        />
        {(sidebarOpen || commandOpen) && (
          <button
            type="button"
            className="sidebar-scrim"
            onClick={() => {
              setSidebarOpen(false)
              setCommandOpen(false)
            }}
            aria-label="Close sidebar"
          />
        )}

        <main className="studio-main">
          <div className="studio-heading">
            {(source !== "live" || testDiscoveryError) && (
              <ConnectionNotice
                connecting={source === "connecting"}
                testsOnly={source === "live"}
                error={testDiscoveryError ?? connectionError}
                onRetry={retryConnection}
              />
            )}
            {activeCommand ? (
              <CommandExecutionHeader
                execution={activeCommand}
                root={
                  nodes.find((node) => node.subject) ??
                  nodes.find(
                    (node) => node.kind === "command" && node.subject !== false
                  )
                }
                running={running}
                onEdit={() => setCommandOpen(true)}
              />
            ) : (
              (source === "live" || selectedRun) && (
                <ExecutionHeader
                  layout={layout}
                  name={selectedRun?.testName ?? selectedTest?.name}
                  definition={definition}
                  run={selectedRun}
                  nodes={nodes}
                  view={view}
                  loading={loading}
                  connected={source === "live"}
                  canRun={
                    source === "live" &&
                    Boolean(selectedTest) &&
                    !loading &&
                    !running &&
                    (Boolean(definition) || Boolean(selectedRun))
                  }
                  error={error}
                  onRun={() => void executeSelectedTest()}
                />
              )
            )}
          </div>
          <MessageGraph
            key={
              activeCommand
                ? "command-execution"
                : (selectedRunId ?? selectedTestId ?? "empty")
            }
            layoutMode={layout}
            nodes={nodes}
            view={view}
            capture={activeCommand?.result?.series?.capture}
            emptyMessage={
              source !== "live"
                ? "Connect to Tracer to load tests and message flows."
                : undefined
            }
          />
        </main>
      </div>
    </TooltipProvider>
  )
}

function operationNodeStatus(
  operation: CommandExecutionResult["operation"]
): MessageGraphNode["status"] {
  if (operation.status === "failed" || operation.status === "indeterminate")
    return operation.status
  if (typeof operation.result === "object" && operation.result !== null) {
    const decision = Reflect.get(operation.result, "decision")
    if (decision === "accepted" || decision === "rejected") return decision
  }
  return operation.status === "completed" ? "indeterminate" : "running"
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
          run.source !== "demo" &&
          !run.runId.startsWith("demo-") &&
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
