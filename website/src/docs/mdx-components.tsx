import type { AnchorHTMLAttributes, ComponentPropsWithoutRef } from "react"
import { Link } from "@tanstack/react-router"
import { ExternalLink } from "lucide-react"
import type { MDXComponents } from "mdx/types"

import { LazyDomainFlow } from "@/components/docs/LazyDomainFlow"
import { LazyMacroCodeExample } from "@/components/docs/LazyMacroCodeExample"
import { LazyProjectStructureExample } from "@/components/docs/LazyProjectStructureExample"
import { MdxCodeBlock } from "@/components/docs/MdxCodeBlock"
import { cn } from "@/lib/utils"

function mdxLink({
  children,
  className,
  href = "",
  ...props
}: AnchorHTMLAttributes<HTMLAnchorElement>) {
  const linkClassName = cn(
    "font-medium text-primary underline decoration-primary/35 underline-offset-4 transition-colors hover:decoration-primary",
    className
  )

  if (href === "/") {
    return (
      <Link {...props} className={linkClassName} to="/">
        {children}
      </Link>
    )
  }

  if (href === "/docs" || href === "/docs/") {
    return (
      <Link {...props} className={linkClassName} to="/docs">
        {children}
      </Link>
    )
  }

  if (href.startsWith("/docs/")) {
    return (
      <Link
        {...props}
        className={linkClassName}
        params={{ _splat: href.slice("/docs/".length) }}
        to="/docs/$"
      >
        {children}
      </Link>
    )
  }

  const isExternal = /^https?:\/\//.test(href)

  return (
    <a
      {...props}
      className={linkClassName}
      href={href}
      rel={isExternal ? "noreferrer" : props.rel}
      target={isExternal ? "_blank" : props.target}
    >
      {children}
      {isExternal ? (
        <ExternalLink
          aria-hidden="true"
          className="ml-1 inline size-3 align-baseline"
        />
      ) : null}
    </a>
  )
}

function mdxTable({ className, ...props }: ComponentPropsWithoutRef<"table">) {
  return (
    <div className="my-6 overflow-x-auto rounded-lg border border-border">
      <table
        className={cn("w-full border-collapse text-left text-sm", className)}
        {...props}
      />
    </div>
  )
}

export const mdxComponents = {
  DomainFlow: LazyDomainFlow,
  MacroExample: LazyMacroCodeExample,
  ProjectStructureExample: LazyProjectStructureExample,
  a: mdxLink,
  blockquote: ({ className, ...props }) => (
    <blockquote
      className={cn(
        "my-6 border-l-2 border-primary/60 pl-4 text-muted-foreground italic",
        className
      )}
      {...props}
    />
  ),
  code: ({ className, ...props }) => (
    <code
      className={cn(
        "rounded-md border border-border bg-muted px-1.5 py-0.5 font-mono text-[0.85em] text-foreground",
        className
      )}
      {...props}
    />
  ),
  h1: ({ className, ...props }) => (
    <h1
      className={cn(
        "scroll-mt-24 text-3xl font-semibold tracking-tight text-foreground sm:text-4xl",
        className
      )}
      {...props}
    />
  ),
  h2: ({ className, ...props }) => (
    <h2
      className={cn(
        "mt-12 scroll-mt-24 border-b border-border/70 pb-3 text-2xl font-semibold tracking-tight text-foreground",
        className
      )}
      {...props}
    />
  ),
  h3: ({ className, ...props }) => (
    <h3
      className={cn(
        "mt-8 scroll-mt-24 text-xl font-semibold tracking-tight text-foreground",
        className
      )}
      {...props}
    />
  ),
  hr: ({ className, ...props }) => (
    <hr className={cn("my-10 border-border", className)} {...props} />
  ),
  li: ({ className, ...props }) => (
    <li className={cn("my-1.5 pl-1", className)} {...props} />
  ),
  ol: ({ className, ...props }) => (
    <ol
      className={cn(
        "my-5 list-decimal space-y-1 pl-6 text-muted-foreground marker:text-primary",
        className
      )}
      {...props}
    />
  ),
  p: ({ className, ...props }) => (
    <p
      className={cn("mt-5 leading-7 text-muted-foreground", className)}
      {...props}
    />
  ),
  pre: MdxCodeBlock,
  table: mdxTable,
  tbody: ({ className, ...props }) => (
    <tbody className={cn("divide-y divide-border/70", className)} {...props} />
  ),
  td: ({ className, ...props }) => (
    <td
      className={cn("px-4 py-3 text-muted-foreground", className)}
      {...props}
    />
  ),
  th: ({ className, ...props }) => (
    <th
      className={cn(
        "bg-muted/60 px-4 py-3 font-medium text-foreground",
        className
      )}
      {...props}
    />
  ),
  thead: ({ className, ...props }) => (
    <thead className={cn("border-b border-border", className)} {...props} />
  ),
  ul: ({ className, ...props }) => (
    <ul
      className={cn(
        "my-5 list-disc space-y-1 pl-6 text-muted-foreground marker:text-primary",
        className
      )}
      {...props}
    />
  ),
} satisfies MDXComponents
