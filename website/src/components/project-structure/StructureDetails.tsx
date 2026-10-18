import { CheckCircle2, FileCode2, FolderOpen, ShieldCheck } from "lucide-react"

import type { StructureNode } from "./types"

type StructureDetailsProps = {
  node: StructureNode
}

export function StructureDetails({ node }: StructureDetailsProps) {
  return (
    <article aria-live="polite" className="flex min-w-0 flex-col p-4">
      <div className="flex items-start gap-3 border-b border-border pb-4">
        <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center border border-border bg-accent text-primary">
          {node.kind === "directory" ? (
            <FolderOpen aria-hidden="true" className="size-5" />
          ) : (
            <FileCode2 aria-hidden="true" className="size-5" />
          )}
        </div>
        <div className="min-w-0">
          <p className="text-[11px] text-primary">{node.role}</p>
          <h3 className="mt-1 text-lg font-semibold wrap-anywhere">
            {node.name}
          </h3>
          <p className="mt-1 font-mono text-[10px] wrap-anywhere text-muted-foreground">
            {node.path}
          </p>
        </div>
      </div>

      <p className="mt-4 text-sm leading-6 text-muted-foreground">
        {node.summary}
      </p>

      <div className="mt-4">
        <h4 className="text-xs font-semibold">Allowed contents</h4>
        <ul className="mt-3 space-y-2">
          {node.allowed.map((item) => (
            <li
              key={item}
              className="flex gap-2 text-xs leading-6 text-muted-foreground"
            >
              <CheckCircle2
                aria-hidden="true"
                className="mt-1 size-3.5 shrink-0 text-success"
              />
              <span>{item}</span>
            </li>
          ))}
        </ul>
      </div>

      <div className="mt-5 border-t border-border pt-4">
        <div className="flex gap-3">
          <ShieldCheck
            aria-hidden="true"
            className="mt-0.5 size-4 shrink-0 text-primary"
          />
          <div>
            <h4 className="text-xs font-semibold">Ownership and checks</h4>
            <p className="mt-2 text-xs leading-6 text-muted-foreground">
              {node.guarantee}
            </p>
          </div>
        </div>
      </div>
    </article>
  )
}
