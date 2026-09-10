import { useEffect, useRef, useState } from "react"
import {
  AlertTriangle,
  ChevronRight,
  FlaskConical,
  History,
  LoaderCircle,
  Send,
} from "lucide-react"

import {
  CommandPanel,
  type CommandExecutionRequest,
} from "@/components/command-panel"
import { MessageGraph } from "@/components/message-graph"
import { StudioSidebar } from "@/components/studio-sidebar"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import {
  CommandSubmissionIndeterminateError,
  getCatalog,
  getFixture,
  getTest,
  listTests,
  submitCommand,
  runTest,
} from "@/lib/api"
import { expectedGraph, operationGraph, reportGraph } from "@/lib/graph"
import {
  SAMPLE_DEFINITIONS,
  SAMPLE_FIXTURE,
  SAMPLE_FIXTURES,
  SAMPLE_GRAPH,
  SAMPLE_TESTS,
} from "@/lib/sample-data"
import type {
  ExpectedMessageNode,
  Fixture,
  MessageGraphNode,
  StoredRun,
  TestDefinitionRevision,
  TestDefinitionSummary,
  TestReport,
  TracerCatalog,
  CommandExecutionResult,
} from "@/lib/types"

const STORED_RUNS_KEY = "rostfrei-tracer-studio-runs-v1"
const MAXIMUM_STORED_RUNS = 16

function shortcutLabel(key: string): string {
  return navigator.userAgent.includes("Mac") ? `⌘⇧${key}` : `Ctrl ⇧ ${key}`
}

function App() {
  const [testsOpen, setTestsOpen] = useState(
    () => window.matchMedia("(min-width: 768px)").matches
  )
  const [runsOpen, setRunsOpen] = useState(false)
  const [commandOpen, setCommandOpen] = useState(false)
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
  const [fixture, setFixture] = useState<Fixture>(SAMPLE_FIXTURE)
  const [nodes, setNodes] = useState<MessageGraphNode[]>(SAMPLE_GRAPH)
  const [runs, setRuns] = useState<StoredRun[]>(readStoredRuns)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState<string>()
  const [catalog, setCatalog] = useState<TracerCatalog>()
  const [commandResult, setCommandResult] = useState<CommandExecutionResult>()
  const [commandError, setCommandError] = useState<string>()
  const [activeCommandLabel, setActiveCommandLabel] = useState<string>()
  const [testStateRevision, setTestStateRevision] = useState(0)
  const viewRevision = useRef(0)
  const definitionsHref =
    catalog?.testRepository?.definitionsHref ??
    catalog?.behavioralTest?.definitionsHref
  const testsAvailable = source !== "live" || Boolean(definitionsHref)

  useEffect(() => {
    const togglePanel = (event: KeyboardEvent) => {
      if (
        !(event.metaKey || event.ctrlKey) ||
        !event.shiftKey ||
        event.altKey
      ) {
        return
      }

      if (event.code === "KeyE" && testsAvailable) {
        event.preventDefault()
        setTestsOpen((open) => !open)
      } else if (event.code === "KeyY") {
        event.preventDefault()
        setRunsOpen((open) => !open)
      } else if (event.code === "KeyK") {
        event.preventDefault()
        setCommandOpen((open) => !open)
      }
    }

    window.addEventListener("keydown", togglePanel)
    return () => window.removeEventListener("keydown", togglePanel)
  }, [testsAvailable])

  useEffect(() => {
    let active = true
    const connect = async () => {
      const connectionRevision = viewRevision.current
      let availableCatalog: TracerCatalog
      try {
        availableCatalog = await getCatalog()
      } catch {
        if (active) setSource("demo")
        return
      }
      if (!active) return
      setCatalog(availableCatalog)
      setSource("live")
      setTests([])
      setSelectedTestId(undefined)
      if (viewRevision.current === connectionRevision) setNodes([])

      const href =
        availableCatalog.testRepository?.definitionsHref ??
        availableCatalog.behavioralTest?.definitionsHref
      if (!href) return
      try {
        const availableTests = await listTests(href)
        if (!active) return
        setTests(availableTests)
        const selected =
          availableTests.find((test) => test.id === SAMPLE_TESTS[0]?.id) ??
          availableTests[0]
        if (!selected) return
        const revision = await getTest(selected.definitionHref)
        const canonicalFixture = await getFixture(
          revision.definition.setup.fixture
        )
        if (!active || viewRevision.current !== connectionRevision) return
        setSelectedTestId(selected.id)
        setDefinition(revision)
        setFixture(canonicalFixture)
        setNodes(expectedGraph(revision.definition, canonicalFixture))
      } catch (testError) {
        if (active) setError(`Tests unavailable: ${errorMessage(testError)}`)
      }
    }
    void connect()
    return () => {
      active = false
    }
  }, [])

  const selectTest = async (test: TestDefinitionSummary) => {
    if (running) return
    const selectionRevision = ++viewRevision.current
    setActiveCommandLabel(undefined)
    setCommandResult(undefined)
    setCommandError(undefined)
    setSelectedTestId(test.id)
    setSelectedRunId(undefined)
    setError(undefined)
    if (source === "live") {
      try {
        const revision = await getTest(test.definitionHref)
        const canonicalFixture = await getFixture(
          revision.definition.setup.fixture
        )
        if (viewRevision.current !== selectionRevision) return
        setDefinition(revision)
        setFixture(canonicalFixture)
        setNodes(expectedGraph(revision.definition, canonicalFixture))
      } catch (selectionError) {
        if (viewRevision.current !== selectionRevision) return
        setError(errorMessage(selectionError))
      }
    } else {
      const revision = SAMPLE_DEFINITIONS[test.id]
      if (revision) {
        const sampleFixture =
          SAMPLE_FIXTURES[revision.definition.setup.fixture] ?? SAMPLE_FIXTURE
        setDefinition(revision)
        setFixture(sampleFixture)
        setNodes(expectedGraph(revision.definition, sampleFixture))
      }
    }
    if (window.matchMedia("(max-width: 767px)").matches) {
      setTestsOpen(false)
    }
  }

  const executeSelectedTest = async () => {
    const selected = tests.find((test) => test.id === selectedTestId)
    if (!selected || running) return
    ++viewRevision.current
    setActiveCommandLabel(undefined)
    setCommandResult(undefined)
    setCommandError(undefined)
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
        const observedNodes = reportGraph(report, fixture)
        setNodes(observedNodes)
        storeRun(report, selected.name, observedNodes)
      } else {
        const demoNodes = await runDemo(definition, fixture, setNodes)
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
      setTestStateRevision((current) => current + 1)
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
    if (running) return
    ++viewRevision.current
    setActiveCommandLabel(undefined)
    setCommandResult(undefined)
    setCommandError(undefined)
    setSelectedRunId(run.runId)
    setSelectedTestId(run.testId)
    setNodes(run.nodes)
    setError(undefined)
    if (window.matchMedia("(max-width: 767px)").matches) {
      setRunsOpen(false)
    }
  }

  const executeCommand = async (request: CommandExecutionRequest) => {
    if (running) return false
    ++viewRevision.current
    setRunning(true)
    setError(undefined)
    setCommandError(undefined)
    setCommandResult(undefined)
    setSelectedRunId(undefined)
    setActiveCommandLabel(
      `${request.commandLabel} · ${request.mode === "test" ? "Test" : "Preview"}`
    )
    const pendingNode: MessageGraphNode = {
      id: `${request.mode}-${crypto.randomUUID()}`,
      kind: "command",
      subject: true,
      name: request.commandId,
      schemaVersion: request.schemaVersion,
      boundedContext: request.context,
      payload: request.payload,
      status: "running",
    }
    setNodes([pendingNode])
    let rotateIdempotencyKey = false

    try {
      const execution = await submitCommand(
        request.mode,
        request.submitHrefTemplate,
        request.schemaVersion,
        request.payloadJson,
        request.mode === "test" ? request.idempotencyKey : undefined
      )
      rotateIdempotencyKey =
        execution.operation.status === "completed" ||
        execution.operation.status === "failed"
      setCommandResult(execution)
      if (execution.series) {
        setNodes(operationGraph(execution.operation, execution.series))
      } else {
        setNodes([
          {
            ...pendingNode,
            response: execution.operation.result ?? execution.operation.failure,
            status: operationNodeStatus(execution.operation),
          },
        ])
      }
    } catch (previewError) {
      const message = errorMessage(previewError)
      const indeterminate =
        previewError instanceof CommandSubmissionIndeterminateError
      setCommandError(message)
      setNodes([
        {
          ...pendingNode,
          status: indeterminate ? "indeterminate" : "failed",
          response: { message },
        },
      ])
    } finally {
      if (request.mode === "test" && rotateIdempotencyKey) {
        setTestStateRevision((current) => current + 1)
      }
      setRunning(false)
    }
    return rotateIdempotencyKey
  }

  const currentLabel =
    activeCommandLabel ??
    (selectedRunId
      ? runs.find((run) => run.runId === selectedRunId)?.testName
      : selectedTestId
        ? definition.definition.name
        : "Choose a command")

  return (
    <TooltipProvider>
      <div className="studio-shell">
        <header className="studio-topbar">
          <div className="studio-logo" aria-label="Rostfrei Tracer Studio">
            <span>rostfrei</span>
            <strong>TRACER STUDIO</strong>
          </div>

          <div className="ml-auto flex min-w-0 items-center gap-2">
            <Badge
              variant={source === "live" ? "live" : "neutral"}
              data-tracer-source={source}
            >
              {source === "live"
                ? "tracer live"
                : source === "demo"
                  ? "demo data"
                  : "connecting"}
            </Badge>
            {running && (
              <Badge variant="live">
                <LoaderCircle className="size-2.5 animate-spin" />
                observing
              </Badge>
            )}
            <span className="hidden max-w-[34vw] truncate font-mono text-[12px] text-white/58 sm:block">
              {currentLabel}
            </span>
          </div>
        </header>

        <StudioSidebar
          testsOpen={testsOpen && testsAvailable}
          runsOpen={runsOpen}
          tests={tests}
          selectedTestId={selectedTestId}
          selectedRunId={selectedRunId}
          runs={runs}
          source={source}
          running={running}
          onCloseTests={() => setTestsOpen(false)}
          onCloseRuns={() => setRunsOpen(false)}
          onSelectTest={(test) => void selectTest(test)}
          onSelectRun={selectRun}
          onRun={() => void executeSelectedTest()}
        />

        <CommandPanel
          open={commandOpen}
          catalog={catalog}
          busy={running}
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

        {((testsOpen && testsAvailable) || runsOpen || commandOpen) && (
          <button
            type="button"
            className="sidebar-scrim"
            onClick={() => {
              setTestsOpen(false)
              setRunsOpen(false)
              setCommandOpen(false)
            }}
            aria-label="Close panels"
          />
        )}

        <main className="studio-main">
          <div className="graph-atmosphere" />
          <MessageGraph nodes={nodes} running={running} />

          <div className="graph-caption" aria-live="polite">
            <span className="graph-caption-dot" />
            <span>{nodes.length} messages</span>
            <span className="text-white/38">/</span>
            <span>
              {nodes.some(
                (node) => !node.context && node.edgeFidelity === "grouped"
              )
                ? "grouped where causation is absent"
                : nodes.some((node) => node.edgeRelationship === "stream-order")
                  ? "stream order + exact causality"
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

        <div
          className="studio-panel-dock"
          role="toolbar"
          aria-label="Studio panels"
        >
          <Button
            variant="ghost"
            size="sm"
            className="studio-panel-trigger"
            onClick={() => setCommandOpen((open) => !open)}
            aria-label={
              commandOpen ? "Close command panel" : "Open command panel"
            }
            aria-controls="studio-command-panel"
            aria-expanded={commandOpen}
            aria-keyshortcuts="Meta+Shift+K Control+Shift+K"
          >
            <Send />
            <span>Command</span>
            <kbd>{shortcutLabel("K")}</kbd>
          </Button>
          <span className="studio-dock-divider" aria-hidden="true" />
          <Button
            variant="ghost"
            size="sm"
            className="studio-panel-trigger"
            onClick={() => setTestsOpen((open) => !open)}
            disabled={!testsAvailable}
            title={
              testsAvailable ? undefined : "This Tracer has no test repository"
            }
            aria-label={
              testsOpen && testsAvailable
                ? "Close tests panel"
                : "Open tests panel"
            }
            aria-controls="studio-tests-panel"
            aria-expanded={testsOpen && testsAvailable}
            aria-keyshortcuts="Meta+Shift+E Control+Shift+E"
          >
            <FlaskConical />
            <span>Tests</span>
            <kbd>{shortcutLabel("E")}</kbd>
          </Button>
          <span className="studio-dock-divider" aria-hidden="true" />
          <Button
            variant="ghost"
            size="sm"
            className="studio-panel-trigger"
            onClick={() => setRunsOpen((open) => !open)}
            aria-label={runsOpen ? "Close runs panel" : "Open runs panel"}
            aria-controls="studio-runs-panel"
            aria-expanded={runsOpen}
            aria-keyshortcuts="Meta+Shift+Y Control+Shift+Y"
          >
            <History />
            <span>Runs</span>
            <kbd>{shortcutLabel("Y")}</kbd>
          </Button>
        </div>
      </div>
    </TooltipProvider>
  )
}

async function runDemo(
  definition: TestDefinitionRevision,
  fixture: Fixture,
  render: (nodes: MessageGraphNode[]) => void
): Promise<MessageGraphNode[]> {
  const preview =
    definition.definition.id === "rent-available-bicycle"
      ? SAMPLE_GRAPH
      : expectedGraph(definition.definition, fixture)
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
      : outcome.rejected.code === "BICYCLE_ALREADY_IN_FLEET"
        ? "The requested bicycle is already part of this fleet."
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
    return value ? (JSON.parse(value) as StoredRun[]) : []
  } catch {
    return []
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "The Tracer request failed"
}

function operationNodeStatus(
  operation: CommandExecutionResult["operation"]
): MessageGraphNode["status"] {
  if (operation.status === "failed") return "failed"
  if (operation.status === "indeterminate") return "indeterminate"
  if (typeof operation.result === "object" && operation.result !== null) {
    const decision = Reflect.get(operation.result, "decision")
    if (decision === "accepted" || decision === "rejected") return decision
  }
  return operation.status === "completed" ? "indeterminate" : "running"
}

export default App
