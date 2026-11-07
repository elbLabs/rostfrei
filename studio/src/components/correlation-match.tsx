import { useId, useState } from "react"
import { Plus, X } from "lucide-react"
import { Input } from "@rostfrei/ui/components/input"
import { Button } from "@/components/ui/button"
import {
  MAX_PAYLOAD_MATCHES,
  formatPayloadMatch,
  parsePayloadMatch,
  payloadMatchProblem,
} from "@/lib/correlation-api"
import {
  blankRow,
  isBlank,
  matchRows,
  type MatchRow,
} from "@/lib/correlation-match"
import type { PayloadMatch } from "@/lib/types"

export function MatchBuilder({
  rows,
  contexts,
  eventTypes,
  disabled,
  note,
  onChange,
}: {
  rows: MatchRow[]
  contexts: string[]
  eventTypes: Map<string, string[]>
  disabled: boolean
  note?: string
  onChange: (rows: MatchRow[]) => void
}) {
  const id = useId()
  const [raw, setRaw] = useState("")
  const [rawProblem, setRawProblem] = useState<string>()
  const update = (key: number, field: keyof PayloadMatch, value: string) =>
    onChange(
      rows.map((row) => (row.key === key ? { ...row, [field]: value } : row))
    )
  const full = rows.length >= MAX_PAYLOAD_MATCHES
  return (
    <fieldset className="correlation-match" disabled={disabled}>
      <legend>Match payload field</legend>
      <datalist id={`${id}-contexts`}>
        {contexts.map((context) => (
          <option key={context} value={context} />
        ))}
      </datalist>
      {rows.map((row, index) => {
        const problem = isBlank(row) ? undefined : payloadMatchProblem(row)
        const number = index + 1
        return (
          <div
            className="match-row"
            key={row.key}
            role="group"
            aria-label={`Match ${number}`}
            data-match-row={number}
          >
            <span>
              <Input
                aria-label={`Match ${number} context`}
                placeholder="context"
                list={`${id}-contexts`}
                value={row.context}
                onChange={(event) =>
                  update(row.key, "context", event.target.value)
                }
              />
              <i aria-hidden="true">/</i>
              <Input
                aria-label={`Match ${number} event type`}
                placeholder="event-type"
                list={`${id}-events-${row.key}`}
                value={row.eventType}
                onChange={(event) =>
                  update(row.key, "eventType", event.target.value)
                }
              />
              <datalist id={`${id}-events-${row.key}`}>
                {(eventTypes.get(row.context.trim()) ?? []).map((type) => (
                  <option key={type} value={type} />
                ))}
              </datalist>
            </span>
            <span>
              <Input
                aria-label={`Match ${number} field`}
                placeholder="field.path"
                value={row.field}
                onChange={(event) =>
                  update(row.key, "field", event.target.value)
                }
              />
              <i aria-hidden="true">=</i>
              <Input
                aria-label={`Match ${number} value`}
                placeholder="value"
                value={row.value}
                onChange={(event) =>
                  update(row.key, "value", event.target.value)
                }
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={`Remove match ${number}`}
                onClick={() =>
                  onChange(
                    rows.filter((candidate) => candidate.key !== row.key)
                  )
                }
              >
                <X />
              </Button>
            </span>
            {problem && <small role="alert">{problem}</small>}
          </div>
        )
      })}
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={full}
        onClick={() => onChange([...rows, blankRow(contexts[0])])}
      >
        <Plus /> Add match
      </Button>
      <details className="match-raw">
        <summary>Raw expression</summary>
        <span>
          <Input
            aria-label="Raw match expression"
            placeholder="context/event-type.field=value"
            value={raw}
            onChange={(event) => {
              setRaw(event.target.value)
              setRawProblem(undefined)
            }}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={full || !raw.trim()}
            onClick={() => {
              const match = parsePayloadMatch(raw.trim())
              if (!match) {
                setRawProblem(
                  "Use <context>/<event-type>.<field>[.<field>…]=<value>"
                )
                return
              }
              onChange([
                ...rows.filter((row) => !isBlank(row)),
                ...matchRows([match]),
              ])
              setRaw("")
            }}
          >
            Add
          </Button>
        </span>
        {rawProblem && <small role="alert">{rawProblem}</small>}
      </details>
      {note && (
        <p className="match-note" role="status">
          {note}
        </p>
      )}
    </fieldset>
  )
}

export function MatchedChips({
  matched,
  disabled,
  onRemove,
  onClear,
}: {
  matched: PayloadMatch[]
  disabled: boolean
  onRemove: (match: PayloadMatch) => void
  onClear: () => void
}) {
  return (
    <section className="matched-chips" aria-label="Applied matches">
      {matched.map((match) => {
        const expression = formatPayloadMatch(match)
        return (
          <span
            className="match-chip"
            key={expression}
            data-matched={expression}
            title={expression}
          >
            <span>
              {match.context}/{match.eventType}.{match.field}
            </span>
            <b>= {match.value}</b>
            <button
              type="button"
              disabled={disabled}
              aria-label={`Remove match ${expression}`}
              onClick={() => onRemove(match)}
            >
              <X size={12} />
            </button>
          </span>
        )
      })}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        disabled={disabled}
        onClick={onClear}
      >
        Clear all
      </Button>
    </section>
  )
}
