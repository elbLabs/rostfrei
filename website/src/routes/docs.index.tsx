import { createFileRoute } from "@tanstack/react-router"

import { MdxContent } from "@/components/docs/MdxContent"
import { loadDocsPage } from "@/docs/registry"

export const Route = createFileRoute("/docs/")({
  loader: async () => {
    const page = await loadDocsPage("")

    if (!page) {
      throw new Error("The documentation introduction is not registered")
    }

    return page
  },
  staleTime: Infinity,
  component: DocsIndexPage,
})

function DocsIndexPage() {
  const page = Route.useLoaderData()

  return (
    <div className="min-w-0">
      <MdxContent component={page.default} />
    </div>
  )
}
