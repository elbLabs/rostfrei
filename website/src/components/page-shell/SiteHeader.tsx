import { Link } from "@tanstack/react-router"

import { GitHubStarButton } from "./GitHubStarButton"

export function SiteHeader() {
  return (
    <header className="border-b border-border/70">
      <div className="mx-auto flex max-w-295 items-center justify-between px-5 py-5 sm:px-8">
        <Link
          className="group inline-flex items-center gap-3"
          to="/"
          aria-label="Rostfrei home"
        >
          <span className="grid size-8 place-items-center border border-primary/50 bg-primary/10 font-mono text-sm font-bold text-primary">
            R
          </span>
          <span className="text-sm font-semibold tracking-[0.18em] uppercase">
            Rostfrei
          </span>
        </Link>

        <nav className="flex items-center gap-5" aria-label="Main navigation">
          <Link
            className="text-sm text-muted-foreground transition-colors hover:text-foreground"
            activeProps={{ className: "text-foreground" }}
            to="/docs"
          >
            Docs
          </Link>
          <GitHubStarButton />
        </nav>
      </div>
    </header>
  )
}
