import { CheckCircle2, FileCode2, FolderOpen, ShieldCheck } from "lucide-react"

import type { StructureNode } from "./types"

type StructureDetailsProps = {
  node: StructureNode
}

export function StructureDetails({ node }: StructureDetailsProps) {
  return (
    <article
      aria-live="polite"
      className="flex min-h-[510px] flex-col p-6 sm:p-9 lg:p-11"
    >
      <div className="flex items-start gap-4 border-b border-white/10 pb-7">
        <div className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-lg border border-[#f2a65a]/30 bg-[#f2a65a]/10 text-[#f2a65a]">
          {node.kind === "directory" ? (
            <FolderOpen aria-hidden="true" className="size-5" />
          ) : (
            <FileCode2 aria-hidden="true" className="size-5" />
          )}
        </div>
        <div className="min-w-0">
          <p className="font-mono text-xs tracking-wide text-[#f2a65a] uppercase">
            {node.role}
          </p>
          <h3 className="mt-1 truncate text-2xl font-semibold tracking-tight text-[#fffaf2] sm:text-3xl">
            {node.name}
          </h3>
          <p className="mt-1 truncate font-mono text-xs text-[#777064]">
            {node.path}
          </p>
        </div>
      </div>

      <p className="mt-7 text-base leading-7 text-[#bdb5a9] sm:text-lg">
        {node.summary}
      </p>

      <div className="mt-8">
        <h4 className="text-sm font-semibold tracking-wide text-[#eee6da]">
          Allowed contents
        </h4>
        <ul className="mt-4 space-y-3">
          {node.allowed.map((item) => (
            <li
              key={item}
              className="flex gap-3 text-sm leading-6 text-[#aaa296]"
            >
              <CheckCircle2
                aria-hidden="true"
                className="mt-1 size-4 shrink-0 text-[#79a27a]"
              />
              <span>{item}</span>
            </li>
          ))}
        </ul>
      </div>

      <div className="mt-auto border-t border-white/10 pt-7">
        <div className="flex gap-4">
          <ShieldCheck
            aria-hidden="true"
            className="mt-0.5 size-5 shrink-0 text-[#f2a65a]"
          />
          <div>
            <h4 className="text-sm font-semibold text-[#eee6da]">
              Structural guarantee
            </h4>
            <p className="mt-2 text-sm leading-6 text-[#91897d]">
              {node.guarantee}
            </p>
          </div>
        </div>
      </div>
    </article>
  )
}
