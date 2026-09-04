import { Link } from "@tanstack/react-router"

export function DocsNotFound() {
  return (
    <div className="min-w-0 py-6">
      <p className="section-kicker">404</p>
      <h1 className="page-title">Documentation page not found</h1>
      <p className="mt-4 max-w-xl leading-7 text-muted-foreground">
        No documentation page matches this address.
      </p>
      <Link className="mt-6 inline-flex text-sm text-link underline" to="/docs">
        Return to the documentation
      </Link>
    </div>
  )
}
