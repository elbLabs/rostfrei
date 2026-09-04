import { ChevronRight, FileCode2, Folder, FolderOpen } from "lucide-react"

import type { StructureNode } from "./types"

type StructureTreeProps = {
  nodes: StructureNode[]
  expanded: Set<string>
  selectedId: string
  onSelect: (node: StructureNode) => void
}

type TreeBranchProps = StructureTreeProps & {
  depth: number
}

function TreeBranch({
  nodes,
  depth,
  expanded,
  selectedId,
  onSelect,
}: TreeBranchProps) {
  return (
    <ul className="space-y-0.5">
      {nodes.map((node) => {
        const isDirectory = node.kind === "directory"
        const isExpanded = isDirectory && expanded.has(node.id)
        const isSelected = selectedId === node.id
        const Icon = isDirectory
          ? isExpanded
            ? FolderOpen
            : Folder
          : FileCode2

        return (
          <li key={node.id}>
            <button
              type="button"
              onClick={() => onSelect(node)}
              aria-expanded={isDirectory ? isExpanded : undefined}
              aria-pressed={isSelected}
              className={`group flex w-full min-w-max items-center gap-2 py-1.5 pr-2 text-left font-mono text-[11px] transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring ${
                isSelected
                  ? "bg-accent text-primary"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
              style={{ paddingLeft: `${depth * 18 + 8}px` }}
            >
              <ChevronRight
                aria-hidden="true"
                className={`size-3.5 shrink-0 transition-transform ${
                  isDirectory ? "opacity-70" : "opacity-0"
                } ${isExpanded ? "rotate-90" : ""}`}
              />
              <Icon
                aria-hidden="true"
                className={`size-4 shrink-0 ${
                  isSelected ? "text-primary" : "text-muted-foreground"
                }`}
              />
              <span className="truncate">{node.name}</span>
            </button>

            {isDirectory && isExpanded && node.children ? (
              <TreeBranch
                nodes={node.children}
                depth={depth + 1}
                expanded={expanded}
                selectedId={selectedId}
                onSelect={onSelect}
              />
            ) : null}
          </li>
        )
      })}
    </ul>
  )
}

export function StructureTree(props: StructureTreeProps) {
  return (
    <nav aria-label="Typed project structure">
      <TreeBranch {...props} depth={0} />
    </nav>
  )
}
