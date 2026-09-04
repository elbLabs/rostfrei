import { Link } from "@tanstack/react-router"
import { Brand } from "@rostfrei/ui/brand"
import { ThemeToggle } from "@rostfrei/ui/theme"

import { GitHubStarButton } from "./GitHubStarButton"

export function SiteHeader() {
  return (
    <header className="site-header">
      <Link to="/" aria-label="Rostfrei home">
        <Brand />
      </Link>
      <nav aria-label="Main navigation">
        <Link to="/" activeOptions={{ exact: true }}>
          Overview
        </Link>
        <Link to="/docs">Docs</Link>
        <GitHubStarButton />
        <ThemeToggle />
      </nav>
    </header>
  )
}
