import { Suspense, lazy, type ReactNode } from "react"

import type { ProjectStructureExampleName } from "@/docs/project-structure-example-data"

const ProjectStructureExample = lazy(() =>
  import("./ProjectStructureExample").then((module) => ({
    default: module.ProjectStructureExample,
  }))
)

export function LazyProjectStructureExample({
  children,
  name,
}: {
  children: ReactNode
  name: ProjectStructureExampleName
}) {
  return (
    <Suspense
      fallback={
        <div className="my-6 grid h-64 place-items-center rounded-xl border border-border bg-muted/20 text-sm text-muted-foreground">
          Loading structure example…
        </div>
      }
    >
      <ProjectStructureExample name={name}>{children}</ProjectStructureExample>
    </Suspense>
  )
}
