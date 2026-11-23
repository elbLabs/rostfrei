import { FlaskConical, LoaderCircle, Play } from "lucide-react"
import { DraggablePanel } from "@/components/draggable-panel"
import { Button } from "@/components/ui/button"
import { NativeSelect } from "@rostfrei/ui/components/native-select"
import { messageFacts } from "@/lib/message-presentation"
import type {
  MessageGraphNode,
  TestDefinitionRevision,
  TestDefinitionSummary,
} from "@/lib/types"

export function TestPanel({
  open,
  tests,
  selectedTestId,
  definition,
  nodes,
  busy,
  running,
  canRun,
  onClose,
  onSelect,
  onRun,
}: {
  open: boolean
  tests: TestDefinitionSummary[]
  selectedTestId?: string
  definition?: TestDefinitionRevision
  nodes: MessageGraphNode[]
  busy: boolean
  running: boolean
  canRun: boolean
  onClose: () => void
  onSelect: (test: TestDefinitionSummary) => void
  onRun: () => void
}) {
  const expectations = definition?.definition.expected.graphs[0]?.nodes ?? []
  const command = expectations.find(
    (node) => node.kind === "command" && !node.parentKey
  )
  const outcome = command?.kind === "command" ? command.outcome : undefined
  const root =
    nodes.find((node) => node.subject) ??
    nodes.find((node) => node.kind === "command" && !node.context)
  const events = expectations.filter((node) => node.kind !== "command").length
  return (
    <DraggablePanel
      id="studio-test-panel"
      className="test-panel"
      label="Run a test"
      icon={FlaskConical}
      open={open}
      initialPosition={{ x: 400, y: 94 }}
      closeLabel="Close test panel"
      onClose={onClose}
    >
      <form
        className="test-panel-form"
        onSubmit={(event) => {
          event.preventDefault()
          if (canRun) onRun()
        }}
      >
        <label htmlFor="studio-test-select">Test</label>
        <NativeSelect
          id="studio-test-select"
          value={selectedTestId ?? ""}
          disabled={busy || !tests.length}
          onChange={(event) => {
            const test = tests.find((test) => test.id === event.target.value)
            if (test) onSelect(test)
          }}
        >
          <option value="" disabled>
            Choose a test
          </option>
          {tests.map((test) => (
            <option key={test.id} value={test.id}>
              {test.name}
            </option>
          ))}
        </NativeSelect>
        {!tests.length ? (
          <p className="command-empty">No behavioral tests are registered.</p>
        ) : (
          <>
            {selectedTestId && definition && (
              <details className="test-definition">
                <summary>Test definition</summary>
                <dl className="scenario-summary">
                  <div>
                    <dt>Given</dt>
                    <dd>{definition.definition.setup.fixture}</dd>
                  </div>
                  <div>
                    <dt>When</dt>
                    <dd>
                      {command?.name ?? root?.name}
                      {root && (
                        <span>
                          {messageFacts(root).map(([key, value]) => (
                            <span className="block" key={key}>
                              {key}: {value}
                            </span>
                          ))}
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
                          {events} expected {events === 1 ? "event" : "events"}
                        </span>
                      )}
                    </dd>
                  </div>
                </dl>
              </details>
            )}
            <p className="execution-note">
              Runs on this host’s isolated Test runtime. Resets its Test state
              to the selected fixture before sending the command.
            </p>
            <Button className="run-button" type="submit" disabled={!canRun}>
              {running ? <LoaderCircle className="animate-spin" /> : <Play />}
              {running ? "Running…" : "Run test"}
            </Button>
          </>
        )}
      </form>
    </DraggablePanel>
  )
}
