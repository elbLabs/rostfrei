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
    <Suspense fallback={children}>
      <ProjectStructureExample key={name} name={name} />
    </Suspense>
  )
}
