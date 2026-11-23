import { useCallback, useEffect, useRef, useState } from "react"
import { FlaskConical, PanelLeft, Send } from "lucide-react"
import { Brand } from "@rostfrei/ui/brand"
import { ThemeToggle } from "@rostfrei/ui/theme"

import { ConnectionNotice } from "@/components/connection-notice"
import {
  CommandPanel,
  type CommandExecutionRequest,
} from "@/components/command-panel"
import {
  CommandExecutionHeader,
  type CommandView,
} from "@/components/command-execution-header"
import {
  CorrelationHeading,
  CorrelationPanel,
  type CorrelationConnection,
  type CorrelationSelection,
} from "@/components/correlation-panel"
import { ExecutionHeader } from "@/components/execution-header"
import { InventoryPanel } from "@/components/inventory-panel"
import { MessageGraph } from "@/components/message-graph"
import { OperatorCommands } from "@/components/operator-commands"
import { RunInspection } from "@/components/run-inspection"
import {
  ObservationPanel,
  type ObservationConnection,
} from "@/components/observation-panel"
import { StudioSidebar } from "@/components/studio-sidebar"
import { TestPanel } from "@/components/test-panel"
import { WorkspaceTabs } from "@/components/workspace-tabs"
import { WorkspaceMap } from "@/components/workspace-map"
import {
  configuredFeatures,
  workspaceViews,
  type StudioFeatures,
  type WorkspaceView,
} from "@/lib/workspace"
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
import {
  correlationGraph,
  expectedGraph,
  observedGraph,
  operationGraph,
  reportGraph,
} from "@/lib/graph"
import { operatorChoices } from "@/lib/operator-api"
import type {
  Fixture,
  CommandExecutionResult,
  MessageGraphNode,
  StoredRun,
  TestDefinitionRevision,
  TestDefinitionSummary,
  TestReport,
  TracerCatalog,
  ObservedFlow,
} from "@/lib/types"
import { cn } from "@/lib/utils"

const STORED_RUNS_KEY = "rostfrei-tracer-studio-runs-v1"
const MAXIMUM_STORED_RUNS = 16
const features = configuredFeatures(
  import.meta.env.VITE_STUDIO_WORKSPACES,
  Boolean(import.meta.env.VITE_TRACER_INSPECTION_TOKEN?.trim())
)

function App() {
  if (!features) {
    return (
      <main className="studio-configuration-error">
        <h1>Studio is not configured</h1>
        <p>
          Remove the unrecognized <code>VITE_STUDIO_WORKSPACES</code> setting
          and restart Studio to enable the default navigation.
        </p>
      </main>
    )
  }
  return <Studio features={features} />
}

function Studio({ features }: { features: StudioFeatures }) {
  const { behavioralTests, initialView } = features
  const [workspace, setWorkspace] = useState<WorkspaceView>(initialView)
  const commandsActive = behavioralTests && workspace === "commands"
  const operatorActive = !behavioralTests && workspace === "commands"
  const testsActive = workspace === "tests"
  const runsActive = workspace === "runs"
  const needsTestDiscovery = testsActive || runsActive || commandsActive
  const observationActive = workspace === "observation"
  const correlationsActive = workspace === "correlations"
  const [correlationSelection, setCorrelationSelection] =
    useState<CorrelationSelection>({ loading: false })
  const [correlationConnection, setCorrelationConnection] =
    useState<CorrelationConnection>("connecting")
  const [observedFlow, setObservedFlow] = useState<ObservedFlow>()
  const [observationExpired, setObservationExpired] = useState(false)
  const [observationConnection, setObservationConnection] =
    useState<ObservationConnection>("connecting")
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [testOpen, setTestOpen] = useState(false)

  const [source, setSource] = useState<"connecting" | "live" | "disconnected">(
    "connecting"
  )
  const [connectionAttempt, setConnectionAttempt] = useState(0)
  const completedAttempt = useRef(-1)
  const [connectionError, setConnectionError] = useState<string>()
  const [testDiscoveryError, setTestDiscoveryError] = useState<string>()
  const [catalog, setCatalog] = useState<TracerCatalog>()
  const [commandOpen, setCommandOpen] = useState(false)
  const [productionOpen, setProductionOpen] = useState(false)
  const productionAvailable =
    commandsActive && catalog && operatorChoices(catalog, "dispatch").length > 0
  const productionActive = productionAvailable && productionOpen
  const [commandResult, setCommandResult] = useState<CommandExecutionResult>()
  const [commandError, setCommandError] = useState<string>()
  const [activeCommand, setActiveCommand] = useState<CommandView>()
  const [commandNodes, setCommandNodes] = useState<MessageGraphNode[]>([])
  const [testStateRevision, setTestStateRevision] = useState(0)
  const [tests, setTests] = useState<TestDefinitionSummary[]>([])
  const [selectedTestId, setSelectedTestId] = useState<string>()
  const [selectedRunId, setSelectedRunId] = useState<string>()
  const [historyRunId, setHistoryRunId] = useState<string>()
  const [inspectingRun, setInspectingRun] = useState(false)
  const [definition, setDefinition] = useState<TestDefinitionRevision>()
  const [nodes, setNodes] = useState<MessageGraphNode[]>([])
  const [runs, setRuns] = useState<StoredRun[]>(() =>
    behavioralTests ? readStoredRuns() : []
  )
  const [executing, setExecuting] = useState<"commands" | "tests">()
  const running = Boolean(executing)
  const commandRunning = executing === "commands"
  const testRunning = executing === "tests"
  const [loading, setLoading] = useState(true)
  const [resultUnavailable, setResultUnavailable] = useState(false)
  const [error, setError] = useState<string>()
  const [storageNote, setStorageNote] = useState<string>()

  const changeView = useCallback(
    (next: WorkspaceView) => {
      if (next === workspace) return
      setWorkspace(next)
      setSidebarOpen(
        next === "runs" || next === "observation" || next === "correlations"
      )
      setCommandOpen(false)
      setProductionOpen(false)
      setTestOpen(false)
      setInspectingRun(false)
    },
    [workspace]
  )

  useEffect(() => {
    if (!behavioralTests) return
    const toggleCommand = (event: KeyboardEvent) => {
      if (
        !event.defaultPrevented &&
        !event.isComposing &&
        !event.repeat &&
        (event.ctrlKey || event.metaKey) &&
        event.shiftKey &&
        !event.altKey &&
        event.code === "KeyK"
      ) {
        event.preventDefault()
        setWorkspace("commands")
        document.getElementById("studio-tab-commands")?.focus()
        setSidebarOpen(false)
        setTestOpen(false)
        setProductionOpen(false)
        setCommandOpen((open) => !open)
      }
    }
    window.addEventListener("keydown", toggleCommand)
    return () => window.removeEventListener("keydown", toggleCommand)
  }, [behavioralTests])

  useEffect(() => {
    if (!needsTestDiscovery || completedAttempt.current === connectionAttempt)
      return
    const controller = new AbortController()
    const { signal } = controller
    const finish = () => {
      if (signal.aborted) return
      completedAttempt.current = connectionAttempt
      setLoading(false)
    }
    const connect = async () => {
      let availableCatalog: TracerCatalog
      try {
        availableCatalog = await getCatalog(signal)
      } catch (connectError) {
        if (!signal.aborted) {
          setSource("disconnected")
          setConnectionError(errorMessage(connectError))
          finish()
        }
        return
      }
      if (signal.aborted) return
      setCatalog(availableCatalog)
      setSource("live")
      const definitionsHref =
        availableCatalog.testRepository?.definitionsHref ??
        availableCatalog.behavioralTest?.definitionsHref
      if (!definitionsHref) {
        finish()
        return
      }
      try {
        const availableTests = await listTests(definitionsHref)
        if (signal.aborted) return
        const selected = availableTests[0]
        if (!selected) {
          setSource("live")
          setTests([])
          return
        }
        const revision = await getTest(selected.definitionHref)
        if (signal.aborted) return
        const fixture = await getFixture(revision.definition.setup.fixture)
        const expectedNodes = expectedGraph(revision.definition, fixture)
        if (signal.aborted) return
        setTests(availableTests)
        setSelectedTestId(selected.id)
        setDefinition(revision)
        setNodes(expectedNodes)
        setSource("live")
      } catch (testError) {
        if (signal.aborted) return
        setTestDiscoveryError(errorMessage(testError))
      } finally {
        finish()
      }
    }
    // Don't start a request from an effect that was immediately cleaned up.
    queueMicrotask(() => {
      if (!signal.aborted) void connect()
    })
    return () => controller.abort()
  }, [connectionAttempt, needsTestDiscovery])

  const retryConnection = () => {
    if (loading || running) return
    setSource("connecting")
    setLoading(true)
    setConnectionError(undefined)
    setTestDiscoveryError(undefined)
    setCatalog(undefined)
    setActiveCommand(undefined)
    setCommandNodes([])
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
    setWorkspace("tests")
    setLoading(true)
    setInspectingRun(false)
    setSelectedTestId(test.id)
    setSelectedRunId(undefined)
    setError(undefined)
    setResultUnavailable(false)
    setDefinition(undefined)
    setNodes([])
    setSidebarOpen(false)
    try {
      const { revision, fixture } = await loadTest(test)
      setDefinition(revision)
      setNodes(expectedGraph(revision.definition, fixture))
    } catch (selectionError) {
      setError(errorMessage(selectionError))
    } finally {
      setLoading(false)
    }
  }

  const executeSelectedTest = async () => {
    const selected = tests.find((test) => test.id === selectedTestId)
    if (!selected || running || loading || source !== "live") return
    setTestOpen(false)
    setInspectingRun(false)
    setWorkspace("tests")
    setExecuting("tests")
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
      setExecuting(undefined)
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
    setWorkspace("runs")
    setHistoryRunId(run.runId)
    setInspectingRun(false)
    setTestOpen(false)
    setSidebarOpen(false)
  }

  const prepareRerun = async (run: StoredRun) => {
    const test = tests.find((test) => test.id === run.testId)
    if (!test || running || loading) return
    await selectTest(test)
    setTestOpen(true)
  }

  const executeCommand = async (
    request: CommandExecutionRequest
  ): Promise<boolean> => {
    if (running || loading || source !== "live") return false
    setWorkspace("commands")
    setExecuting("commands")
    setCommandError(undefined)
    setCommandResult(undefined)
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
    setCommandNodes([pending])
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
      setCommandNodes(
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
      setCommandNodes([
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
      setExecuting(undefined)
    }
    return rotateKey
  }

  const selectedRun = runsActive
    ? (runs.find((run) => run.runId === historyRunId) ?? runs[0])
    : runs.find((run) => run.runId === selectedRunId)
  const selectedTest = tests.find((test) => test.id === selectedTestId)
  const canRunTest =
    source === "live" &&
    Boolean(selectedTest) &&
    !loading &&
    !running &&
    (Boolean(definition) || Boolean(selectedRun))
  const inspectionRun =
    (testsActive || runsActive) && !testRunning && inspectingRun
      ? selectedRun
      : undefined
  const view = correlationsActive
    ? "retained"
    : observationActive
      ? "observed"
      : (commandsActive ? commandRunning : testsActive && testRunning)
        ? "running"
        : commandsActive
          ? activeCommand?.error
            ? "unavailable"
            : "observed"
          : testsActive && resultUnavailable
            ? "unavailable"
            : selectedRun
              ? "observed"
              : "expected"
  const displayNodes = correlationsActive
    ? correlationSelection.chain
      ? correlationGraph(correlationSelection.chain.refs)
      : []
    : observationActive
      ? observedFlow
        ? observedGraph(observedFlow.messageSeries)
        : []
      : commandsActive
        ? commandNodes
        : runsActive
          ? (selectedRun?.nodes ?? [])
          : nodes
  const mode = workspaceViews.find((mode) => mode.id === workspace)!
  const graphMode = !operatorActive && "emptyTitle" in mode ? mode : undefined
  const sidebarLabel = testsActive
    ? "Test library"
    : runsActive
      ? "Recent runs"
      : observationActive
        ? "Flows"
        : "Find correlation"
  const floatingGraph = Boolean(
    graphMode && !productionActive && !inspectionRun
  )

  const workspaceToolbar = graphMode && (
    <header className="workspace-toolbar">
      <div className="workspace-identity">
        <span className="workspace-icon">
          <mode.icon size={21} />
        </span>
        <div>
          <div className="workspace-title">
            <h1>{graphMode.label}</h1>
            <span>{productionActive ? "Production" : graphMode.scope}</span>
          </div>
          <p>
            {productionActive
              ? "Dispatch a command to Production."
              : graphMode.description}
          </p>
        </div>
      </div>
      <div className="workspace-actions">
        {!commandsActive && (
          <Button
            variant="outline"
            size="sm"
            className="sidebar-toggle"
            onClick={() => setSidebarOpen((open) => !open)}
            aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
            aria-expanded={sidebarOpen}
            aria-controls={
              observationActive
                ? "studio-observation-panel"
                : correlationsActive
                  ? "studio-correlation-panel"
                  : "studio-navigation"
            }
          >
            <PanelLeft />
            <span>{sidebarLabel}</span>
          </Button>
        )}
        {productionAvailable && (
          <Button
            className="production-dispatch-trigger"
            variant="outline"
            size="sm"
            onClick={() => {
              setCommandOpen(false)
              setProductionOpen((open) => !open)
            }}
            aria-controls={
              productionActive ? "studio-production-dispatch" : undefined
            }
            aria-expanded={productionActive}
          >
            {productionActive
              ? "Close Production dispatch"
              : "Production dispatch"}
          </Button>
        )}
        {commandsActive && !productionActive && (
          <Button
            className="command-panel-trigger"
            size="sm"
            disabled={!catalog}
            onClick={() => setCommandOpen((open) => !open)}
            aria-controls="studio-command-panel"
            aria-expanded={commandOpen}
            aria-label={
              commandOpen ? "Close command panel" : "Open command panel"
            }
            aria-keyshortcuts="Meta+Shift+K Control+Shift+K"
          >
            <Send />
            <span>Send command</span>
          </Button>
        )}
        {testsActive && (
          <Button
            className="test-panel-trigger"
            size="sm"
            disabled={!catalog}
            onClick={() => setTestOpen((open) => !open)}
            aria-controls="studio-test-panel"
            aria-expanded={testOpen}
            aria-label={testOpen ? "Close test panel" : "Open test panel"}
          >
            <FlaskConical />
            <span>Run a test</span>
          </Button>
        )}
        {runsActive && selectedRun && (
          <Button
            className="saved-run-rerun"
            size="sm"
            disabled={
              source !== "live" ||
              running ||
              loading ||
              !tests.some((test) => test.id === selectedRun.testId)
            }
            onClick={() => void prepareRerun(selectedRun)}
          >
            <FlaskConical /> Run again
          </Button>
        )}
        {floatingGraph && (testsActive || runsActive) && selectedRun && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => setInspectingRun(true)}
          >
            Run summary
          </Button>
        )}
      </div>
    </header>
  )

  return (
    <TooltipProvider>
      <div
        className={cn(
          "studio-shell",
          floatingGraph && "has-floating-graph",
          (observationActive || correlationsActive) && "observation-active",
          sidebarOpen && "sidebar-is-open"
        )}
      >
        <header className="studio-topbar">
          <div className="studio-logo" aria-label="Rostfrei Tracer Studio">
            <Brand compact className="studio-brand" />
            <strong>Tracer Studio</strong>
          </div>
          <WorkspaceMap
            value={workspace}
            behavioralTests={behavioralTests}
            onChange={changeView}
          />
          {needsTestDiscovery || observationActive || correlationsActive ? (
            <div
              className="connection-status"
              data-source={
                observationActive
                  ? observationConnection
                  : correlationsActive
                    ? correlationConnection
                    : source
              }
              data-tracer-source={needsTestDiscovery ? source : undefined}
            >
              <span className="connection-dot" />
              <span className="connection-label">
                {observationActive
                  ? `Observation ${observationConnection}`
                  : correlationsActive
                    ? `Correlations ${correlationConnection}`
                    : source === "connecting"
                      ? "Connecting"
                      : source === "live"
                        ? productionActive
                          ? "Connected"
                          : "Isolated Test"
                        : "Disconnected"}
              </span>
              {runsActive && selectedRun && (
                <span className="connection-detail">Saved run</span>
              )}
            </div>
          ) : (
            <div className="studio-topbar-spacer" />
          )}
          <ThemeToggle className="studio-theme-toggle" />
        </header>
        <WorkspaceTabs
          behavioralTests={behavioralTests}
          value={workspace}
          onChange={changeView}
        />

        <StudioSidebar
          open={sidebarOpen && (testsActive || runsActive)}
          mode={runsActive ? "runs" : "tests"}
          tests={tests}
          selectedTestId={selectedTestId}
          selectedRunId={runsActive ? selectedRun?.runId : selectedRunId}
          runs={runs}
          busy={!runsActive && (running || loading)}
          storageNote={storageNote}
          onClose={() => setSidebarOpen(false)}
          onSelectTest={(test) => void selectTest(test)}
          onSelectRun={selectRun}
        />
        <ObservationPanel
          open={sidebarOpen && observationActive}
          active={observationActive}
          testEnabled={
            behavioralTests ||
            Boolean(import.meta.env.VITE_TRACER_TOKEN?.trim())
          }
          onClose={() => setSidebarOpen(false)}
          onSelect={() => setSidebarOpen(false)}
          onFlow={setObservedFlow}
          onExpired={setObservationExpired}
          onStatus={setObservationConnection}
        />
        <CorrelationPanel
          open={sidebarOpen && correlationsActive}
          active={correlationsActive}
          onClose={() => setSidebarOpen(false)}
          onSelect={() => setSidebarOpen(false)}
          onSelection={setCorrelationSelection}
          onStatus={setCorrelationConnection}
        />
        {behavioralTests && (
          <CommandPanel
            active={needsTestDiscovery}
            open={commandsActive && commandOpen}
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
        )}
        {catalog && (
          <TestPanel
            open={testsActive && testOpen}
            tests={tests}
            selectedTestId={selectedTestId}
            definition={definition}
            nodes={nodes}
            busy={running || loading}
            running={testRunning}
            canRun={canRunTest}
            onClose={() => setTestOpen(false)}
            onSelect={(test) => void selectTest(test)}
            onRun={() => void executeSelectedTest()}
          />
        )}
        {(sidebarOpen ||
          (commandsActive && commandOpen) ||
          (testsActive && testOpen)) && (
          <button
            type="button"
            className="sidebar-scrim"
            onClick={() => {
              setSidebarOpen(false)
              setCommandOpen(false)
              setTestOpen(false)
            }}
            aria-label="Close sidebar"
          />
        )}

        <main
          id="studio-workspace"
          className={cn(
            "studio-main",
            (!graphMode || productionActive) && "workspace-standalone",
            floatingGraph && "workspace-graph"
          )}
          role="tabpanel"
          aria-labelledby={`studio-tab-${workspace}`}
          data-workspace={workspace}
          tabIndex={0}
        >
          {!floatingGraph && workspaceToolbar}
          <div
            className={cn(
              "workspace-content",
              inspectionRun && "is-inspecting-run"
            )}
          >
            {operatorActive ? (
              <OperatorCommands />
            ) : productionActive ? (
              <div id="studio-production-dispatch">
                <OperatorCommands productionOnly controlCatalog={catalog} />
              </div>
            ) : workspace === "infrastructure" ||
              workspace === "domain" ||
              workspace === "read-models" ? (
              <InventoryPanel view={workspace} />
            ) : inspectionRun ? (
              <RunInspection
                key={inspectionRun.runId}
                run={inspectionRun}
                canRun={
                  source === "live" &&
                  tests.some((test) => test.id === inspectionRun.testId) &&
                  !loading &&
                  !running
                }
                onRun={() => void prepareRerun(inspectionRun)}
                onFullGraph={() => setInspectingRun(false)}
              />
            ) : graphMode ? (
              <MessageGraph
                toolbar={workspaceToolbar}
                summary={
                  <div
                    className="studio-heading"
                    tabIndex={0}
                    role="region"
                    aria-label="Flow summary"
                  >
                    {(commandsActive || testsActive || runsActive) &&
                      (source !== "live" || testDiscoveryError) && (
                        <ConnectionNotice
                          connecting={source === "connecting"}
                          testsOnly={source === "live"}
                          error={testDiscoveryError ?? connectionError}
                          onRetry={retryConnection}
                        />
                      )}
                    {correlationsActive ? (
                      <CorrelationHeading
                        selection={correlationSelection}
                        status={correlationConnection}
                      />
                    ) : observationActive ? (
                      observedFlow && (
                        <section
                          className="observation-heading"
                          aria-live="polite"
                        >
                          <div>
                            <h1>
                              {observedFlow?.name ??
                                "Observe application traffic"}
                            </h1>
                            <p>
                              {observedFlow ? (
                                <>
                                  {observedFlow.application} (
                                  {observedFlow.scope})
                                  <code className="block">
                                    {observedFlow.correlationId}
                                  </code>
                                </>
                              ) : (
                                "Choose a flow from the observation sidebar."
                              )}
                            </p>
                          </div>
                          <p className="observation-evidence">
                            {observedFlow
                              ? `${observedFlow.fidelity === "exact" ? "Exact" : "Grouped"} observation. Capture is partial.${observationExpired ? " Retained snapshot." : ""}${observedFlow.truncated ? " Capture truncated." : ""}${observedFlow.conflicted ? " Conflicting evidence." : ""}`
                              : observationConnection}
                            <span>
                              {observationExpired
                                ? "This graph was evicted or reset; select another flow for live updates."
                                : "Rolling window; silence does not establish completion."}
                            </span>
                          </p>
                        </section>
                      )
                    ) : commandsActive ? (
                      activeCommand && (
                        <CommandExecutionHeader
                          execution={activeCommand}
                          root={
                            commandNodes.find((node) => node.subject) ??
                            commandNodes.find(
                              (node) =>
                                node.kind === "command" &&
                                node.subject !== false
                            )
                          }
                          running={commandRunning}
                        />
                      )
                    ) : (
                      (source === "live" || selectedRun) && (
                        <ExecutionHeader
                          name={
                            selectedRun?.testName ??
                            (testsActive ? selectedTest?.name : undefined)
                          }
                          run={selectedRun}
                          nodes={displayNodes}
                          view={view}
                          loading={testsActive && loading}
                          connected={source === "live"}
                          canRun={
                            runsActive
                              ? tests.some(
                                  (test) => test.id === selectedRun?.testId
                                ) && source === "live"
                              : canRunTest
                          }
                          error={testsActive ? error : undefined}
                        />
                      )
                    )}
                  </div>
                }
                key={
                  correlationsActive
                    ? `correlation-${correlationSelection.correlationId ?? "empty"}`
                    : observationActive
                      ? `observation-${observedFlow?.scope ?? "empty"}-${observedFlow?.id ?? "empty"}`
                      : commandsActive
                        ? "command-execution"
                        : `${workspace}-${selectedRun?.runId ?? (testsActive ? selectedTestId : "empty")}`
                }
                nodes={displayNodes}
                view={view}
                capture={
                  correlationsActive
                    ? undefined
                    : observationActive
                      ? observedFlow
                        ? {
                            fidelity: observedFlow.fidelity,
                            settled: false,
                            settledFor: "0ms",
                          }
                        : undefined
                      : commandsActive
                        ? activeCommand?.result?.series?.capture
                        : selectedRun?.capture
                }
                emptyTitle={graphMode.emptyTitle}
                emptyMessage={
                  correlationsActive
                    ? correlationSelection.loading
                      ? "Reading retained events…"
                      : correlationSelection.chain
                        ? "No events returned in this retained-history read."
                        : graphMode.emptyMessage
                    : observationActive
                      ? graphMode.emptyMessage
                      : source !== "live" && !runsActive
                        ? "Connect to Tracer to load tests and message flows."
                        : graphMode.emptyMessage
                }
              />
            ) : null}
          </div>
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
      .map((run) => ({ ...run, capture: readCapture(run.capture) }))
  } catch {
    return []
  }
}

function readCapture(value: unknown): StoredRun["capture"] {
  if (!value || typeof value !== "object") return undefined
  const capture = value as Record<string, unknown>
  if (
    (capture.fidelity !== "exact" && capture.fidelity !== "grouped") ||
    typeof capture.settled !== "boolean" ||
    typeof capture.settledFor !== "string"
  )
    return undefined
  return {
    fidelity: capture.fidelity,
    settled: capture.settled,
    settledFor: capture.settledFor,
    ...(typeof capture.note === "string" ? { note: capture.note } : {}),
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "The Tracer request failed"
}

export default App
