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
    <div className="mx-auto w-full max-w-5xl px-6 py-10 lg:px-10 lg:py-14">
      <MdxContent component={page.default} />
    </div>
  )
}
