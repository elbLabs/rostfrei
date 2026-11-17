import { useState } from "react"
import { EvidenceFlow, type EvidenceMessage } from "@rostfrei/ui/evidence"
import { useTheme } from "@rostfrei/ui/theme"
import { Moon, Sun } from "lucide-react"

import { MessageGraph } from "@/components/message-graph"
import type { MessageGraphNode } from "@/lib/types"

// Illustrative data only. Both renderers receive the same explicit causal chain.
const nodes: MessageGraphNode[] = [
  {
    id: "study-command",
    kind: "command",
    name: "rent-bicycle",
    subject: true,
    status: "accepted",
    payload: { bicycle_id: "bike-42" },
  },
  {
    id: "study-domain",
    parentId: "study-command",
    edgeFidelity: "exact",
    edgeRelationship: "causation",
    kind: "domain-event",
    name: "bicycle-rented",
    payload: { bicycle_id: "bike-42" },
  },
  {
    id: "study-integration",
    parentId: "study-domain",
    edgeFidelity: "exact",
    edgeRelationship: "causation",
    kind: "integration-event",
    name: "rental-started",
    payload: { bicycle_id: "bike-42" },
  },
]

const messages: EvidenceMessage[] = nodes.map((node) => ({
  id: node.id,
  kind: {
    command: "Command",
    "domain-event": "Domain event",
    "integration-event": "Integration event",
  }[node.kind],
  tone: node.kind,
  name: node.name,
  detail: node.kind === "command" ? "Accepted" : "Observed",
}))
const edges = nodes.flatMap((node) =>
  node.parentId ? [{ from: node.parentId, to: node.id }] : []
)

const designs = [
  {
    id: "hairline",
    letter: "A",
    name: "Hairline + rail",
    description:
      "A quiet grey boundary. A fine colour rail keeps the message kind visible.",
  },
  {
    id: "soft",
    letter: "B",
    name: "Soft outline",
    description:
      "Rounded corners and a muted kind-coloured outline. A little softer on the canvas.",
  },
  {
    id: "frame",
    letter: "C",
    name: "Split frame",
    description:
      "Square edges and a divided type header. More structure for scanning a busy flow.",
  },
] as const

type Design = (typeof designs)[number]
type Surface = "full" | "compact"

function DesignOption({
  design,
  surface,
  preferred,
  onPrefer,
}: {
  design: Design
  surface: Surface
  preferred: boolean
  onPrefer: () => void
}) {
  const [selectedId, setSelectedId] = useState("study-command")

  return (
    <article
      className={`node-study-option node-study-${design.id}`}
      aria-labelledby={`title-${design.id}`}
      data-design={design.id}
    >
      <div className="node-study-description">
        <h2 id={`title-${design.id}`}>
          <span>{design.letter}</span> {design.name}
        </h2>
        <p>{design.description}</p>
        <button
          type="button"
          className="node-study-choice"
          aria-pressed={preferred}
          onClick={onPrefer}
        >
          {preferred
            ? `Preferred: ${design.letter}`
            : `Prefer ${design.letter}`}
        </button>
      </div>
      <div className="node-study-preview">
        {surface === "full" ? (
          <MessageGraph nodes={nodes} view="observed" />
        ) : (
          <EvidenceFlow
            messages={messages}
            edges={edges}
            selectedId={selectedId}
            onSelect={setSelectedId}
          />
        )}
      </div>
    </article>
  )
}

export default function NodeDesigns() {
  const { resolvedTheme, setTheme } = useTheme()
  const [surface, setSurface] = useState<Surface>("full")
  const [preferred, setPreferred] = useState<Design["id"]>()
  const preference = designs.find((design) => design.id === preferred)

  return (
    <main className="node-study">
      <header className="node-study-header">
        <div>
          <a href="/">Back to Studio</a>
          <h1>A little more definition.</h1>
          <p>Three bordered-node directions, on the same open grey canvas.</p>
        </div>
        <button
          type="button"
          className="node-study-theme"
          aria-label={`Switch to ${resolvedTheme === "light" ? "dark" : "light"} theme`}
          onClick={() => setTheme(resolvedTheme === "light" ? "dark" : "light")}
        >
          {resolvedTheme === "light" ? <Moon size={16} /> : <Sun size={16} />}
          {resolvedTheme === "light" ? "Dark" : "Light"}
        </button>
      </header>

      <div className="node-study-toolbar">
        <div
          className="node-study-surfaces"
          role="group"
          aria-label="Node presentation"
        >
          <button
            type="button"
            aria-pressed={surface === "full"}
            onClick={() => setSurface("full")}
          >
            Full Studio graph
          </button>
          <button
            type="button"
            aria-pressed={surface === "compact"}
            onClick={() => setSurface("compact")}
          >
            Compact Studio / MCP
          </button>
        </div>
        <p>Click to select. Hover or use Tab to compare states.</p>
      </div>
      <p className="node-study-disclosure">
        Illustrative messages and causal links. This study uses local sample
        data only.
      </p>

      <div className="node-study-options" data-surface={surface}>
        {designs.map((design) => (
          <DesignOption
            key={design.id}
            design={design}
            surface={surface}
            preferred={preferred === design.id}
            onPrefer={() => setPreferred(design.id)}
          />
        ))}
      </div>

      <footer className="node-study-footer">
        <p>
          <strong>Studio uses A.</strong> Clear boundaries with the least visual
          weight. The colour rail survives selection and scales well to the
          compact reader.
        </p>
        <p role="status">
          {preference
            ? `Your preference: ${preference.letter}, ${preference.name}. Marked in this preview only.`
            : "Choose a preference after trying both sizes."}
        </p>
        <p className="node-study-mcp-note">
          The compact view uses the shared EvidenceFlow component already used
          by Studio and the experimental MCP Apps reader. Both now use A.
          Preference buttons only mark this comparison.
        </p>
      </footer>
    </main>
  )
}
