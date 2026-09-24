import { useEffect, useRef, useState } from "react"
import { ArrowLeft, Check, Copy, ScanSearch, X } from "lucide-react"

import { Button } from "@/components/ui/button"
import { kindLabels, messageState } from "@/lib/message-presentation"
import type { FlowView, MessageGraphNode } from "@/lib/types"

interface MessageInspectorProps {
  node?: MessageGraphNode
  parent?: MessageGraphNode
  view: FlowView
  hidden: boolean
  closable: boolean
  onBack: () => void
}

export function MessageInspector({
  node,
  parent,
  view,
  hidden,
  closable,
  onBack,
}: MessageInspectorProps) {
  return (
    <aside
      className="message-inspector"
      aria-label="Message details"
      tabIndex={-1}
      hidden={hidden}
      onKeyDown={(event) => {
        if (event.key === "Escape" && closable) {
          event.preventDefault()
          onBack()
        }
      }}
    >
      <div className="inspector-heading">
        <Button className="inspector-back" variant="ghost" onClick={onBack}>
          <ArrowLeft /> Back to flow
        </Button>
        <span>Message details</span>
        {closable ? (
          <Button
            className="inspector-close"
            variant="ghost"
            size="icon"
            onClick={onBack}
            aria-label="Close inspector"
          >
            <X />
          </Button>
        ) : (
          <ScanSearch size={16} aria-hidden="true" />
        )}
      </div>
      {node ? (
        <div className="inspector-scroll" key={node.id}>
          <header className="inspector-message-heading">
            <span className={`message-kind kind-${node.kind}`}>
              {kindLabels[node.kind]}
            </span>
            <h2>{node.name}</h2>
            <span
              className="message-state"
              data-status={
                view === "expected" || node.context ? "idle" : node.status
              }
            >
              {messageState(node, view)}
            </span>
          </header>

          <section className="inspector-section">
            <h3>{node.context ? "Fixture context" : "Relationship"}</h3>
            <p className="relationship-description">
              {node.context
                ? `Given domain event${node.streamVersion === undefined ? "" : ` · stream version ${node.streamVersion}`}. This is setup state for the test.`
                : node.edgeRelationship === "context"
                  ? "Root command. Evaluates the fixture state; the fixture is not its cause."
                  : parent && node.edgeFidelity === "exact"
                    ? `${view === "expected" ? "Expected after" : view === "predicted" ? "Predicted after" : "Caused by"} ${parent.name}.`
                    : node.kind === "command" &&
                        node.subject !== false &&
                        !node.causationId
                      ? "Root command for this execution."
                      : view === "expected"
                        ? "No parent specified in the expectation."
                        : "Associated with this run. No resolved causal parent is available."}
            </p>
          </section>

          <section className="inspector-section">
            <h3>{node.kind === "command" ? "Request" : "Payload"}</h3>
            <PayloadList payload={node.payload} />
          </section>

          {node.kind === "command" && (
            <section className="inspector-section" data-command-response>
              <h3>{view === "expected" ? "Expected outcome" : "Response"}</h3>
              {view === "expected" ? (
                node.expectedOutcome ? (
                  <PayloadList
                    payload={
                      node.expectedOutcome === "accepted"
                        ? { decision: "accepted" }
                        : {
                            decision: "rejected",
                            ...node.expectedOutcome.rejected,
                          }
                    }
                  />
                ) : (
                  <p>No outcome specified.</p>
                )
              ) : node.response === undefined ? (
                <p>
                  {node.status === "running"
                    ? "Waiting for the command response…"
                    : "Response redacted or unavailable."}
                </p>
              ) : (
                <PayloadList
                  payload={node.response}
                  collapseDuplicateRejectionDetails
                />
              )}
            </section>
          )}

          <section className="inspector-section">
            <h3>Metadata</h3>
            <dl className="payload-list">
              <Metadata
                label="Schema version"
                value={String(node.schemaVersion)}
              />
              <Metadata label="Context" value={node.boundedContext} />
              <Metadata label="Aggregate type" value={node.aggregateType} />
              <Metadata label="Aggregate ID" value={node.aggregateId} />
              <Metadata
                label="Stream version"
                value={
                  node.streamVersion === undefined
                    ? undefined
                    : String(node.streamVersion)
                }
              />
            </dl>
            {(node.messageId || node.causationId) && (
              <div className="identity-controls">
                {node.messageId && (
                  <CopyIdentityButton
                    label="message ID"
                    value={node.messageId}
                  />
                )}
                {node.causationId && (
                  <CopyIdentityButton
                    label="cause ID"
                    value={node.causationId}
                  />
                )}
              </div>
            )}
          </section>
        </div>
      ) : (
        <div className="inspector-empty">
          <ScanSearch />
          <h2>Explore a message</h2>
          <p>
            Select a card to inspect its payload, outcome, and causal
            relationship.
          </p>
        </div>
      )}
    </aside>
  )
}

function Metadata({ label, value }: { label: string; value?: string }) {
  return value === undefined ? null : (
    <div className="payload-row">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  )
}

function CopyIdentityButton({
  label,
  value,
}: {
  label: string
  value: string
}) {
  const [copied, setCopied] = useState(false)
  const [failed, setFailed] = useState(false)
  const resetTimer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(resetTimer.current), [])
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setFailed(false)
      window.clearTimeout(resetTimer.current)
      resetTimer.current = window.setTimeout(() => setCopied(false), 1400)
    } catch {
      setFailed(true)
    }
  }
  return (
    <div>
      <Button
        variant="outline"
        size="sm"
        data-copy-identity={label}
        aria-label={`Copy ${label}`}
        onClick={() => void copy()}
      >
        {copied ? <Check /> : <Copy />}
        {copied ? "Copied" : label}
      </Button>
      {failed && (
        <p role="status">
          Copy unavailable: <code>{value}</code>
        </p>
      )}
    </div>
  )
}

interface PayloadRow {
  path: string
  value: string
  kind: string
}

function PayloadList({
  payload,
  collapseDuplicateRejectionDetails = false,
}: {
  payload: unknown
  collapseDuplicateRejectionDetails?: boolean
}) {
  if (payload === undefined)
    return <p className="payload-unavailable">Redacted or unavailable.</p>
  let rows = flattenPayload(payload)
  if (collapseDuplicateRejectionDetails) {
    const values = new Map(rows.map((row) => [row.path, row]))
    rows = rows.filter((row) => {
      const match = /^(rejection\.)?details\.(code|message)$/.exec(row.path)
      if (!match) return true
      const primary = values.get(`${match[1] ?? ""}${match[2]}`)
      return primary?.kind !== row.kind || primary.value !== row.value
    })
  }
  return (
    <dl className="payload-list">
      {rows.map((row, index) => (
        <div className="payload-row" key={`${row.path}-${index}`}>
          <dt title={row.path}>{row.path}</dt>
          <dd data-kind={row.kind}>{row.value}</dd>
        </div>
      ))}
    </dl>
  )
}

function flattenPayload(
  value: unknown,
  path = "value",
  rows: PayloadRow[] = []
): PayloadRow[] {
  if (value === null) {
    rows.push({ path, value: "null", kind: "null" })
  } else if (Array.isArray(value)) {
    if (!value.length) rows.push({ path, value: "Empty list", kind: "empty" })
    value.forEach((item, index) =>
      flattenPayload(item, `${path}[${index}]`, rows)
    )
  } else if (typeof value === "object") {
    const entries = Object.entries(value)
    if (!entries.length)
      rows.push({ path, value: "Empty object", kind: "empty" })
    entries.forEach(([key, item]) =>
      flattenPayload(item, path === "value" ? key : `${path}.${key}`, rows)
    )
  } else {
    rows.push({ path, value: String(value), kind: typeof value })
  }
  return rows
}
