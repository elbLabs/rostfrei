import { ChevronRight, FileCode2, Folder, FolderOpen } from "lucide-react"
import { useId, useState } from "react"

import {
  projectStructureExamples,
  type ProjectStructureExampleName,
} from "@/docs/project-structure-example-data"
import { cn } from "@/lib/utils"
import { MdxCodeBlock } from "./MdxCodeBlock"

// Loaded with this lazy documentation component, from the same checkout as the docs.
const sources = import.meta.glob<string>(
  "../../../../examples/bike-rental/src/domain/**/*.rs",
  { eager: true, query: "?raw", import: "default" }
)

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
  name,
}: {
  name: ProjectStructureExampleName
}) {
  const example = projectStructureExamples[name]
  const rows = buildTreeRows(example.files)
  const [selectedFile, setSelectedFile] = useState<string>(example.activeFile)
  const [expanded, setExpanded] = useState(
    () =>
      new Set(
        rows.filter((row) => row.type === "directory").map((row) => row.path)
      )
  )
  const sourceId = useId()
  const sourcePath = `examples/bike-rental/${example.directory}/${selectedFile}`
  const source = sources[`../../../../${sourcePath}`]
  if (source === undefined)
    throw new Error(`Missing example source: ${sourcePath}`)

  function toggleDirectory(path: string) {
    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }

  const visibleRows = rows.filter((row) =>
    row.path
      .split("/")
      .slice(0, -1)
      .every((_, index, parents) =>
        expanded.has(parents.slice(0, index + 1).join("/"))
      )
  )

  return (
    <div className="project-file-browser" data-project-structure-example={name}>
      <nav
        className="project-file-tree"
        aria-label={`Example files in ${example.root}`}
      >
        <div className="mb-3 flex items-center gap-2 font-mono text-[13px] text-muted-foreground">
          <FolderOpen className="size-3.5" aria-hidden="true" />
          {example.root}
        </div>
        <ul>
          {visibleRows.map((row) => {
            const isDirectory = row.type === "directory"
            const isActive = !isDirectory && row.path === selectedFile
            const isExpanded = expanded.has(row.path)
            const Icon = isDirectory
              ? isExpanded
                ? FolderOpen
                : Folder
              : FileCode2

            return (
              <li key={row.path}>
                <button
                  type="button"
                  aria-label={isDirectory ? `Toggle ${row.path}` : row.path}
                  aria-expanded={isDirectory ? isExpanded : undefined}
                  aria-pressed={!isDirectory ? isActive : undefined}
                  aria-controls={!isDirectory ? sourceId : undefined}
                  data-source-path={
                    !isDirectory
                      ? `examples/bike-rental/${example.directory}/${row.path}`
                      : undefined
                  }
                  onClick={() =>
                    isDirectory
                      ? toggleDirectory(row.path)
                      : setSelectedFile(row.path)
                  }
                  className={cn(
                    "flex min-h-8 w-full min-w-max items-center gap-2 rounded pr-2 text-left font-mono text-[13px]",
                    isActive
                      ? "bg-accent text-accent-foreground"
                      : "text-muted-foreground hover:bg-muted hover:text-foreground"
                  )}
                  style={{ paddingLeft: `${row.depth * 14 + 4}px` }}
                >
                  <ChevronRight
                    aria-hidden="true"
                    className={cn(
                      "size-3 shrink-0",
                      !isDirectory && "invisible",
                      isExpanded && "rotate-90"
                    )}
                  />
                  <Icon aria-hidden="true" className="size-3.5 shrink-0" />
                  {row.name}
                </button>
              </li>
            )
          })}
        </ul>
      </nav>

      <section
        id={sourceId}
        className="project-file-source"
        aria-label="Selected file"
        data-selected-source={sourcePath}
      >
        <div className="project-file-source-heading">
          <code aria-live="polite" title={sourcePath}>
            {selectedFile}
          </code>
          <a
            href={`https://github.com/elbLabs/rostfrei/blob/main/${sourcePath}`}
            className="text-link"
          >
            GitHub
          </a>
        </div>
        <MdxCodeBlock key={sourcePath}>
          <code className="language-rust">{source.trimEnd()}</code>
        </MdxCodeBlock>
      </section>
    </div>
  )
}
