import type { ComponentType } from "react"
import { MDXProvider } from "@mdx-js/react"
import type { MDXProps } from "mdx/types"

import { mdxComponents } from "@/docs/mdx-components"
import { cn } from "@/lib/utils"

export interface MdxContentProps {
  component: ComponentType<MDXProps>
  className?: string
}

export function MdxContent({ component: Content, className }: MdxContentProps) {
  return (
    <MDXProvider components={mdxComponents}>
      <article
        className={cn(
          "docs-article max-w-full min-w-0 pb-8 [&>*:first-child]:mt-0",
          className
        )}
      >
        <Content components={mdxComponents} />
      </article>
    </MDXProvider>
  )
}
