import { useEffect, useRef, useState } from "react"
import {
  EvidenceDisclosure,
  EvidenceFlow,
  EvidenceSummary,
} from "@rostfrei/ui/evidence"
import { MessageInspector } from "@/components/message-inspector"
import { Button } from "@/components/ui/button"
import { kindLabels, messageState } from "@/lib/message-presentation"
import type { StoredRun } from "@/lib/types"

export function RunInspection({
  run,
  canRun,
  onRun,
  onFullGraph,
}: {
  run: StoredRun
  canRun: boolean
  onRun: () => void
  onFullGraph: () => void
}) {
  const [selectedId, setSelectedId] = useState<string>()
  const rootRef = useRef<HTMLDivElement>(null)
  const trigger = useRef<HTMLElement | null>(null)
  const messages = run.nodes.filter((node) => !node.context)
  const root =
    messages.find((node) => node.subject === true) ??
    messages.find((node) => node.kind === "command" && node.subject !== false)
  const selected = messages.find((node) => node.id === selectedId)
  const outcome = root ? messageState(root, "observed") : "Outcome unavailable"
  const fixtureCount = run.nodes.length - messages.length
  const messageIds = new Set(messages.map((node) => node.id))
  const unresolved = messages.filter(
    (node) =>
      node !== root &&
      (!node.parentId ||
        !messageIds.has(node.parentId) ||
        node.hideIncomingEdge ||
        node.edgeFidelity !== "exact" ||
        node.edgeRelationship === "context" ||
        node.edgeRelationship === "stream-order")
  ).length
  const diagnosticCodes = [
    ...new Set(run.diagnostics?.map((diagnostic) => diagnostic.code)),
  ]
  const displayed = messages.slice(0, 6)
  const displayedIds = new Set(displayed.map((node) => node.id))
  const edges = displayed.flatMap((node) =>
    node.parentId &&
    displayedIds.has(node.parentId) &&
    node.edgeFidelity === "exact" &&
    !node.hideIncomingEdge &&
    node.edgeRelationship !== "context" &&
    node.edgeRelationship !== "stream-order"
      ? [{ from: node.parentId, to: node.id }]
      : []
  )

  useEffect(() => {
    if (selectedId)
      rootRef.current?.querySelector<HTMLElement>(".message-inspector")?.focus()
  }, [selectedId])

  return (
    <div className="run-inspection" ref={rootRef}>
      <Button
        className="run-summary-back"
        variant="ghost"
        onClick={onFullGraph}
      >
        Back to graph
      </Button>
      <div className="execution-result" data-status={run.status}>
        <EvidenceSummary
          title={run.testName}
          facts={[
            {
              label: "Test",
              value: run.status === "passed" ? "Passed" : "Failed",
              tone: run.status === "passed" ? "positive" : "negative",
            },
            {
              label: "Command",
              value:
                root?.status === "accepted" || root?.status === "rejected"
                  ? root.status === "accepted"
                    ? "Accepted"
                    : "Rejected"
                  : outcome,
              tone:
                root?.status === "accepted"
                  ? "positive"
                  : root?.status === "rejected"
                    ? "warning"
                    : "neutral",
            },
            {
              label: "Capture",
              value: run.capture
                ? `${run.capture.fidelity === "exact" ? "Exact" : "Grouped"}, ${run.capture.settled ? "settled" : "not settled"}`
                : "Unavailable",
              tone:
                !run.capture ||
                !run.capture.settled ||
                run.capture.fidelity === "grouped"
                  ? "warning"
                  : "neutral",
            },
          ]}
        >
          <p className="run-evidence-note">
            {run.capture
              ? `Settle window: ${run.capture.settledFor}. `
              : "No capture-window or settling metadata. "}
            Downstream completion is not established.
          </p>
          {(run.capture?.note ||
            diagnosticCodes.length > 0 ||
            unresolved > 0 ||
            !run.diagnostics) && (
            <div className="run-evidence-quality">
              {run.capture?.note && <p>{run.capture.note}</p>}
              {unresolved > 0 && (
                <p>
                  {unresolved}{" "}
                  {unresolved === 1 ? "message has" : "messages have"} no
                  resolved causal parent in the saved evidence.
                </p>
              )}
              {diagnosticCodes.length > 0 && (
                <p>Diagnostics: {diagnosticCodes.join(", ")}</p>
              )}
              {!run.diagnostics && <p>Diagnostics unavailable.</p>}
            </div>
          )}
        </EvidenceSummary>
      </div>

      <section className="run-flow" aria-label="Recorded message flow">
        <div className="run-flow-heading">
          <h2>Message flow</h2>
        </div>
        <EvidenceFlow
          selectedId={selectedId}
          edges={edges}
          messages={displayed.map((node) => {
            return {
              id: node.id,
              kind: kindLabels[node.kind],
              tone: node.kind,
              name: node.name,
              detail: edges.some((edge) => edge.to === node.id)
                ? undefined
                : node === root
                  ? undefined
                  : node.parentId &&
                      messageIds.has(node.parentId) &&
                      node.edgeFidelity === "exact" &&
                      !node.hideIncomingEdge
                    ? "Cause outside this preview"
                    : "No resolved causal parent",
            }
          })}
          onSelect={(id) => {
            trigger.current =
              document.activeElement instanceof HTMLElement
                ? document.activeElement
                : null
            setSelectedId(id)
          }}
        />
        {messages.length > 6 && (
          <p className="run-evidence-note">
            Showing 6 of {messages.length} observed messages. Open the full
            graph for the rest.
          </p>
        )}
        {!messages.length && (
          <p>No observed messages were saved with this run.</p>
        )}
      </section>

      {selected && (
        <MessageInspector
          key={selected.id}
          node={selected}
          parent={run.nodes.find((node) => node.id === selected.parentId)}
          view="observed"
          hidden={false}
          closable
          progressive
          onBack={() => {
            setSelectedId(undefined)
            requestAnimationFrame(() => trigger.current?.focus())
          }}
        />
      )}

      {Boolean(run.diagnostics?.length) && (
        <EvidenceDisclosure title="Diagnostic details">
          <ul className="run-diagnostics">
            {run.diagnostics!.map((diagnostic, index) => (
              <li key={index}>
                <strong>{diagnostic.code}</strong>
                <p>{diagnostic.message}</p>
                <code>{diagnostic.path}</code>
              </li>
            ))}
          </ul>
        </EvidenceDisclosure>
      )}
      <EvidenceDisclosure title="Scenario and fixture context">
        <dl className="run-scenario">
          <div>
            <dt>Given</dt>
            <dd>
              {run.definition?.definition.setup.fixture ??
                `${fixtureCount} fixture events`}
            </dd>
          </div>
          <div>
            <dt>When</dt>
            <dd>{root?.name ?? "Command unavailable"}</dd>
          </div>
          <div>
            <dt>Then</dt>
            <dd>Compared with the saved test definition.</dd>
          </div>
        </dl>
        <p>
          {fixtureCount} fixture{" "}
          {fixtureCount === 1 ? "event is" : "events are"} setup context, not
          effects of this command.
        </p>
        <Button variant="link" onClick={onFullGraph}>
          Explore fixture and expected messages
        </Button>
      </EvidenceDisclosure>
      <EvidenceDisclosure title="Run details">
        <dl className="run-scenario">
          <div>
            <dt>Run ID</dt>
            <dd>
              <code>{run.runId}</code>
            </dd>
          </div>
          <div>
            <dt>Saved at</dt>
            <dd>{run.createdAt}</dd>
          </div>
          <div>
            <dt>Source</dt>
            <dd>
              {run.capture
                ? "Saved Tracer report with separately retained finite message-series capture."
                : "Tracer run report saved in this browser; no finite capture metadata."}
            </dd>
          </div>
        </dl>
      </EvidenceDisclosure>
      <EvidenceDisclosure title="Run again">
        <p>
          Uses the current test definition and resets isolated Test state to its
          fixture.
        </p>
        <Button className="run-button" disabled={!canRun} onClick={onRun}>
          Run again
        </Button>
        {!canRun && (
          <p>Connect to Tracer with this test available to run it again.</p>
        )}
      </EvidenceDisclosure>
    </div>
  )
}
