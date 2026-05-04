import { FileCode2, Folder, FolderOpen } from "lucide-react"
import type { ReactNode } from "react"

import {
  projectStructureExamples,
  type ProjectStructureExampleName,
} from "@/docs/project-structure-example-data"
import { cn } from "@/lib/utils"

interface TreeRow {
  readonly depth: number
  readonly name: string
  readonly path: string
  readonly type: "directory" | "file"
}

function buildTreeRows(files: readonly string[]) {
  const rows: TreeRow[] = []
  const seenDirectories = new Set<string>()

  for (const path of files) {
    const parts = path.split("/")

    for (let index = 0; index < parts.length - 1; index += 1) {
      const directoryPath = parts.slice(0, index + 1).join("/")

      if (!seenDirectories.has(directoryPath)) {
        seenDirectories.add(directoryPath)
        rows.push({
          depth: index,
          name: parts[index],
          path: directoryPath,
          type: "directory",
        })
      }
    }

    rows.push({
      depth: parts.length - 1,
      name: parts.at(-1) ?? path,
      path,
      type: "file",
    })
  }

  return rows
}

export function ProjectStructureExample({
  children,
  name,
}: {
  children: ReactNode
  name: ProjectStructureExampleName
}) {
  const example = projectStructureExamples[name]
  const rows = buildTreeRows(example.files)

  return (
    <div className="my-6 overflow-hidden rounded-xl border border-border bg-[#100e0a] shadow-lg shadow-black/10 lg:grid lg:grid-cols-[13rem_minmax(0,1fr)]">
      <aside className="border-b border-border bg-[#14110d] lg:border-r lg:border-b-0">
        <div className="flex h-10 items-center gap-2 border-b border-border px-3 font-mono text-[10px] tracking-[0.12em] text-muted-foreground uppercase">
          <FolderOpen className="size-3.5" aria-hidden="true" />
          {example.root}
        </div>
        <div className="overflow-x-auto p-2">
          {rows.map((row) => {
            const isActive = row.path === example.activeFile
            const Icon = row.type === "directory" ? Folder : FileCode2

            return (
              <div
                aria-current={isActive ? "true" : undefined}
                className={cn(
                  "flex h-7 min-w-max items-center gap-2 rounded-md pr-2 font-mono text-xs",
                  isActive
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground"
                )}
                key={`${row.type}:${row.path}`}
                style={{ paddingLeft: `${row.depth * 14 + 8}px` }}
              >
                <Icon
                  aria-hidden="true"
                  className={cn(
                    "size-3.5 shrink-0",
                    row.type === "file" && "text-primary"
                  )}
                />
                {row.name}
              </div>
            )
          })}
        </div>
      </aside>

      <section className="min-w-0 [&_[data-slot=mdx-code-block]]:my-0 [&_[data-slot=mdx-code-block]]:rounded-none [&_[data-slot=mdx-code-block]]:border-0">
        {children}
      </section>
    </div>
  )
}
