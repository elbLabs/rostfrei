import { createFileRoute, notFound } from "@tanstack/react-router"

import { MdxContent } from "@/components/docs/MdxContent"
import { DocsNotFound } from "@/components/docs/DocsNotFound"
import { loadDocsPage } from "@/docs/registry"

export const Route = createFileRoute("/docs/$")({
  loader: async ({ params }) => {
    if (!params._splat) {
      throw notFound()
    }

    const page = await loadDocsPage(params._splat)

    if (!page) {
      throw notFound()
    }

    return page
  },
  staleTime: Infinity,
  component: DocsContentPage,
  notFoundComponent: DocsNotFound,
})

function DocsContentPage() {
  const page = Route.useLoaderData()

  return (
    <div className="mx-auto w-full max-w-5xl px-6 py-10 lg:px-10 lg:py-14">
      <MdxContent component={page.default} />
    </div>
  )
}
