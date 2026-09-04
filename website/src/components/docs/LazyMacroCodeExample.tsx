import { Suspense, lazy } from "react"

import type { MacroExampleName } from "@/docs/macro-code-data"

const MacroCodeExample = lazy(() =>
  import("./MacroCodeExample").then((module) => ({
    default: module.MacroCodeExample,
  }))
)

export function LazyMacroCodeExample({ name }: { name: MacroExampleName }) {
  return (
    <Suspense
      fallback={
        <div className="my-6 grid h-48 place-items-center rounded-xl border border-border bg-muted/20 text-sm text-muted-foreground">
          Loading code example…
        </div>
      }
    >
      <MacroCodeExample key={name} name={name} />
    </Suspense>
  )
}
