import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react"
import type { EvidenceMessage } from "./evidence"

export interface EvidenceEdge {
  from: string
  to: string
}

export function EvidenceFlow({
  messages,
  edges,
  selectedId,
  onSelect,
}: {
  messages: readonly EvidenceMessage[]
  edges: readonly EvidenceEdge[]
  selectedId?: string | null
  onSelect: (id: string) => void
}) {
  const root = useRef<HTMLDivElement>(null)
  const marker = `evidence-arrow-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`
  const [paths, setPaths] = useState<
    readonly { from: string; to: string; path: string }[]
  >([])
  const byId = new Map(messages.map((message) => [message.id, message]))
  const links = edges.filter((edge) => byId.has(edge.from) && byId.has(edge.to))
  const positions = flowPositions(messages, links)
  const columns = Math.max(
    1,
    ...positions.map((position) => position.column + 1)
  )

  useLayoutEffect(() => {
    const element = root.current
    if (!element) return
    let frame = 0
    const measure = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        const bounds = element.getBoundingClientRect()
        const nodes = new Map(
          [
            ...element.querySelectorAll<HTMLElement>("[data-evidence-message]"),
          ].map(
            (node) =>
              [
                node.dataset.evidenceMessage,
                node.getBoundingClientRect(),
              ] as const
          )
        )
        setPaths(
          edges.flatMap((edge, index) => {
            const source = nodes.get(edge.from)
            const target = nodes.get(edge.to)
            if (!source || !target) return []
            const sy = source.top + source.height / 2 - bounds.top
            const ty = target.top + target.height / 2 - bounds.top
            const sx = source.right - bounds.left
            const tx = target.left - bounds.left
            const adjacentBelow =
              Math.abs(source.left - target.left) < 1 &&
              source.bottom <= target.top &&
              ![...nodes.values()].some(
                (node) =>
                  node !== source &&
                  node !== target &&
                  node.top >= source.bottom &&
                  node.bottom <= target.top
              )
            const lane = 4 + (16 * (index + 1)) / (edges.length + 1)
            const path = adjacentBelow
              ? `M ${source.left + source.width / 2 - bounds.left} ${source.bottom - bounds.top} L ${target.left + target.width / 2 - bounds.left} ${target.top - bounds.top - 4}`
              : target.left > source.right
                ? `M ${sx} ${sy} C ${sx + 24} ${sy}, ${tx - 24} ${ty}, ${tx - 4} ${ty}`
                : edge.from === edge.to
                  ? `M ${sx} ${sy} C ${sx + 24} ${sy - 40}, ${sx + 24} ${sy + 40}, ${sx + 4} ${sy + 12}`
                  : `M ${source.left - bounds.left} ${sy} H ${lane} V ${ty} H ${tx - 4}`
            return [{ ...edge, path }]
          })
        )
      })
    }
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    element
      .querySelectorAll("[data-evidence-message]")
      .forEach((node) => observer.observe(node))
    measure()
    return () => {
      cancelAnimationFrame(frame)
      observer.disconnect()
    }
  }, [messages, edges])

  return (
    <div
      className="rf-evidence-flow"
      style={{ "--flow-columns": columns } as CSSProperties}
    >
      <div className="rf-evidence-flow-scroll">
        <div className="rf-evidence-flow-body" ref={root}>
          <svg className="rf-evidence-flow-edges" aria-hidden="true">
            <defs>
              <marker
                id={marker}
                viewBox="0 0 10 10"
                refX="9"
                refY="5"
                markerWidth="7"
                markerHeight="7"
                orient="auto"
              >
                <path
                  d="M 1 1 L 9 5 L 1 9"
                  className="rf-evidence-flow-arrow"
                />
              </marker>
            </defs>
            {paths.map((edge, index) => (
              <path
                key={`${edge.from}-${edge.to}-${index}`}
                d={edge.path}
                data-evidence-edge
                data-from={edge.from}
                data-to={edge.to}
                markerEnd={`url(#${marker})`}
              />
            ))}
          </svg>
          <ol className="rf-evidence-messages" aria-label="Message flow">
            {messages.map((message, index) => {
              const parents = links
                .filter((edge) => edge.to === message.id)
                .map((edge) => byId.get(edge.from)!.name)
              return (
                <li
                  key={message.id}
                  data-evidence-message={message.id}
                  style={{
                    gridColumn: positions[index].column + 1,
                    gridRow: positions[index].row + 1,
                  }}
                >
                  <button
                    type="button"
                    data-tone={message.tone}
                    aria-label={`${message.kind} ${message.name}`}
                    aria-describedby={
                      parents.length ? `${marker}-cause-${index}` : undefined
                    }
                    aria-pressed={selectedId === message.id}
                    onClick={() => onSelect(message.id)}
                  >
                    <span className="rf-evidence-kind">{message.kind}</span>
                    <strong>{message.name}</strong>
                    {parents.length > 0 && (
                      <span
                        id={`${marker}-cause-${index}`}
                        className="rf-evidence-sr-only"
                      >
                        Caused by {parents.join(", ")}
                      </span>
                    )}
                    {message.detail && (
                      <span className="rf-evidence-message-detail">
                        {message.detail}
                      </span>
                    )}
                  </button>
                </li>
              )
            })}
          </ol>
        </div>
      </div>
    </div>
  )
}

function flowPositions(
  messages: readonly EvidenceMessage[],
  edges: readonly EvidenceEdge[]
) {
  const degree = new Map(messages.map((message) => [message.id, 0]))
  const outgoing = new Map<string, string[]>()
  const columns = new Map(messages.map((message) => [message.id, 0]))
  for (const edge of edges) {
    degree.set(edge.to, degree.get(edge.to)! + 1)
    outgoing.set(edge.from, [...(outgoing.get(edge.from) ?? []), edge.to])
  }
  const queue = messages
    .filter((message) => degree.get(message.id) === 0)
    .map((message) => message.id)
  for (let index = 0; index < queue.length; index++) {
    const id = queue[index]
    for (const child of outgoing.get(id) ?? []) {
      columns.set(child, Math.max(columns.get(child)!, columns.get(id)! + 1))
      degree.set(child, degree.get(child)! - 1)
      if (degree.get(child) === 0) queue.push(child)
    }
  }
  const rows = new Map<number, number>()
  return messages.map((message) => {
    const column = columns.get(message.id)!
    const row = rows.get(column) ?? 0
    rows.set(column, row + 1)
    return { column, row }
  })
}
