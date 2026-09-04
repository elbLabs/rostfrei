import { Link } from "@tanstack/react-router"

export function SiteFooter() {
  return (
    <footer className="site-footer">
      <p>Rostfrei · Open source under EUPL-1.2 · Built by elbtech.dev</p>
      <nav aria-label="Footer navigation" className="flex flex-wrap gap-4">
        <Link to="/docs">Documentation</Link>
        <a href="https://github.com/elbLabs/rostfrei">GitHub</a>
        <a href="https://github.com/elbLabs/rostfrei/blob/main/CHANGELOG.md">
          Changelog
        </a>
        <a href="https://github.com/elbLabs/rostfrei/blob/main/LICENSE">
          License
        </a>
      </nav>
    </footer>
  )
}
