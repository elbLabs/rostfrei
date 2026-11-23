import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

export type GraphCardFact = readonly [label: string, value: string]

type GraphCardProps = {
  kind: string
  icon: ReactNode
  kindTag?: ReactNode
  label: string
  facts: readonly GraphCardFact[]
  footer: string
  footerStatus?: string
  selected?: boolean
  className?: string
  accessibleLabel: string
  title?: string
  onClick: () => void
  attributes?: { [name: `data-${string}`]: string }
}

// Flow messages and domain declarations share presentation, not semantics.
export function GraphCard({
  kind,
  icon,
  kindTag,
  label,
  facts,
  footer,
  footerStatus,
  selected,
  className,
  accessibleLabel,
  title,
  onClick,
  attributes,
}: GraphCardProps) {
  return (
    <button
      type="button"
      className={cn(
        "message-card nodrag nopan",
        selected && "message-card-selected",
        className
      )}
      onClick={onClick}
      aria-label={accessibleLabel}
      aria-pressed={selected}
      title={title}
      {...attributes}
    >
      <span className="message-card-kind">
        {icon}
        {kind}
        {kindTag}
      </span>
      <strong className="message-card-name" title={label}>
        {label}
      </strong>
      <span className="message-card-facts">
        {facts.map(([key, value]) => (
          <span key={key} title={`${key}: ${value}`}>
            <span>{key}</span> <b>{value}</b>
          </span>
        ))}
      </span>
      <span className="message-card-footer" data-status={footerStatus}>
        {footer}
        <span aria-hidden="true">↗</span>
      </span>
    </button>
  )
}
