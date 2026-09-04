import { Outlet, createFileRoute } from "@tanstack/react-router"

import { DocsLayout } from "@/components/docs/DocsLayout"
import { DocsNotFound } from "@/components/docs/DocsNotFound"

export const Route = createFileRoute("/docs")({
  component: DocsPage,
  notFoundComponent: DocsNotFound,
})

function DocsPage() {
  return (
    <DocsLayout>
      <Outlet />
    </DocsLayout>
  )
}
