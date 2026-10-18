import { useState } from "react"

import { StructureDetails } from "./StructureDetails"
import { StructureTree } from "./StructureTree"
import {
  DEFAULT_EXPANDED_IDS,
  DEFAULT_SELECTED_ID,
  STRUCTURE,
  findStructureNode,
} from "./structure-data"
import type { StructureNode } from "./types"

export function ProjectStructureSection() {
  const [selectedId, setSelectedId] = useState(DEFAULT_SELECTED_ID)
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(DEFAULT_EXPANDED_IDS)
  )
  const selected = findStructureNode(STRUCTURE, selectedId) ?? STRUCTURE[0]

  function selectNode(node: StructureNode) {
    setSelectedId(node.id)
    if (node.kind !== "directory") return

    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(node.id)) next.delete(node.id)
      else next.add(node.id)
      return next
    })
  }

  return (
    <section
      aria-labelledby="project-structure-title"
      className="content-section"
    >
      <div className="mx-auto max-w-7xl">
        <div className="mb-5">
          <p className="section-kicker">Project structure</p>
          <h2 id="project-structure-title" className="section-heading">
            Make ownership visible. Keep it checked.
          </h2>
          <p className="mt-3 text-sm leading-6 text-muted-foreground">
            Explore the directory conventions for declarations and behavior.{" "}
            <code>cargo rostfrei check</code> validates their placement and
            implementation targets. This tree shows selected modules from the
            bike-rental example.
          </p>
        </div>

        <div className="min-w-0 overflow-hidden border border-border bg-card">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-muted px-4 py-2">
            <span className="text-xs font-semibold">Bike-rental project</span>
            <code className="font-mono text-[11px] text-muted-foreground">
              cargo rostfrei check
            </code>
          </div>

          <div className="grid lg:grid-cols-[minmax(0,0.92fr)_minmax(0,1.08fr)]">
            <div className="max-h-[510px] min-w-0 overflow-auto border-b border-border bg-muted p-3 lg:border-r lg:border-b-0">
              <StructureTree
                nodes={STRUCTURE}
                expanded={expanded}
                selectedId={selectedId}
                onSelect={selectNode}
              />
            </div>
            <StructureDetails node={selected} />
          </div>
        </div>
      </div>
    </section>
  )
}
