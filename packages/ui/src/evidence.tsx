import { useId, type ReactNode } from "react"

export { EvidenceFlow, type EvidenceEdge } from "./evidence-flow"

export interface EvidenceFact {
  label: string
  value: string
  tone?: "neutral" | "positive" | "negative" | "warning"
}

export function EvidenceSummary({
  title,
  facts,
  children,
}: {
  title: string
  facts: readonly EvidenceFact[]
  children?: ReactNode
}) {
  const id = useId()
  return (
    <section className="rf-evidence-summary" aria-labelledby={id}>
      <h1 id={id}>{title}</h1>
      <dl className="rf-evidence-facts">
        {facts.map((fact, index) => (
          <div
            key={`${fact.label}-${index}`}
            data-tone={fact.tone ?? "neutral"}
          >
            <dt>{fact.label}</dt>
            <dd>{fact.value}</dd>
          </div>
        ))}
      </dl>
      {children}
    </section>
  )
}

export function EvidenceDisclosure({
  title,
  children,
  defaultOpen = false,
}: {
  title: string
  children: ReactNode
  defaultOpen?: boolean
}) {
  return (
    <details className="rf-evidence-disclosure" open={defaultOpen || undefined}>
      <summary>{title}</summary>
      <div className="rf-evidence-disclosure-content">{children}</div>
    </details>
  )
}

export interface EvidenceMessage {
  id: string
  kind: string
  tone?: "command" | "domain-event" | "integration-event"
  name: string
  detail?: string
}

export function EvidenceMessageList({
  messages,
  selectedId,
  onSelect,
}: {
  messages: readonly EvidenceMessage[]
  selectedId?: string | null
  onSelect: (id: string) => void
}) {
  return (
    <ol className="rf-evidence-messages" aria-label="Messages">
      {messages.map((message) => (
        <li key={message.id} data-evidence-message={message.id}>
          <button
            type="button"
            data-tone={message.tone}
            aria-pressed={selectedId === message.id}
            aria-label={`${message.kind} ${message.name}`}
            onClick={() => onSelect(message.id)}
          >
            <span className="rf-evidence-kind">{message.kind}</span>
            <strong>{message.name}</strong>
            {message.detail && (
              <span className="rf-evidence-message-detail">
                {message.detail}
              </span>
            )}
          </button>
        </li>
      ))}
    </ol>
  )
}
