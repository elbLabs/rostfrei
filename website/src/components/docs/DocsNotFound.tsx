import { Link } from "@tanstack/react-router"

export function DocsNotFound() {
  return (
    <div className="mx-auto w-full max-w-5xl px-6 py-16 lg:px-10">
      <p className="font-mono text-xs tracking-[0.14em] text-primary uppercase">
        404
      </p>
      <h1 className="mt-3 text-3xl font-semibold tracking-tight">
        Documentation page not found
      </h1>
      <p className="mt-4 max-w-xl leading-7 text-muted-foreground">
        This page does not exist yet, or its address has changed.
      </p>
      <Link
        className="mt-6 inline-flex text-sm font-medium text-primary underline decoration-primary/35 underline-offset-4 hover:decoration-primary"
        to="/docs"
      >
        Return to the documentation
      </Link>
    </div>
  )
}
