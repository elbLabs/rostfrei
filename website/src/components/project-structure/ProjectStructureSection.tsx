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
      className="bg-[#f3eee6] px-4 py-16 text-[#221f1a] sm:px-6 sm:py-24 lg:px-8"
    >
      <div className="mx-auto max-w-7xl">
        <div className="mb-8 max-w-3xl sm:mb-10">
          <p className="mb-3 font-mono text-xs font-semibold tracking-[0.18em] text-[#9a5729] uppercase">
            Convention, checked
          </p>
          <h2
            id="project-structure-title"
            className="text-3xl font-semibold tracking-[-0.035em] text-[#201d18] sm:text-5xl"
          >
            The filesystem becomes part of the type system.
          </h2>
          <p className="mt-5 max-w-2xl text-base leading-7 text-[#625b51] sm:text-lg">
            One representative path shows how Rostfrei gives every domain role a
            predictable home, then verifies the relationships encoded by that
            structure.
          </p>
        </div>

        <div className="overflow-hidden rounded-[26px] border border-[#332f28] bg-[#1a1815] text-[#f4eee5] shadow-[0_24px_70px_rgba(44,34,23,0.18)]">
          <div className="flex items-center justify-between border-b border-white/10 px-5 py-3 sm:px-7">
            <div className="flex items-center gap-2" aria-hidden="true">
              <span className="size-2.5 rounded-full bg-[#dd7754]" />
              <span className="size-2.5 rounded-full bg-[#d8aa52]" />
              <span className="size-2.5 rounded-full bg-[#668f69]" />
            </div>
            <code className="font-mono text-[11px] tracking-wide text-[#827b70]">
              cargo rostfrei check
            </code>
          </div>

          <div className="grid lg:grid-cols-[minmax(0,0.92fr)_minmax(0,1.08fr)]">
            <div className="max-h-[650px] overflow-auto border-b border-white/10 p-4 sm:p-6 lg:border-r lg:border-b-0">
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
