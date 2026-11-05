import { Link } from "@tanstack/react-router"

export function SiteFooter() {
  return (
    <footer className="site-footer">
      <p>
        Built by <a href="https://elbtech.ai">elbtech</a>
      </p>
      <nav aria-label="Footer navigation" className="flex flex-wrap gap-4">
        <Link to="/docs/$" params={{ _splat: "reference" }}>
          Documentation index
        </Link>
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
