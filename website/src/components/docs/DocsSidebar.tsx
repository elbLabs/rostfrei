import { Link, useLocation } from "@tanstack/react-router"
import { ChevronRight } from "lucide-react"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@rostfrei/ui/components/collapsible"

import {
  docsNavigation,
  type DocsNavigationEntry,
  type DocsNavigationItem,
} from "@/docs/navigation"
import { withoutBasePath } from "@/lib/base-path"

function activeEntry(entry: DocsNavigationEntry, pathname: string): boolean {
  return "items" in entry
    ? entry.items.some((item) => activeEntry(item, pathname))
    : pathname === (entry.slug ? `/docs/${entry.slug}` : "/docs")
}

function DocsLink({ item }: { item: DocsNavigationItem }) {
  return item.slug ? (
    <Link
      to="/docs/$"
      params={{ _splat: item.slug }}
      activeOptions={{ exact: true }}
    >
      {item.title}
    </Link>
  ) : (
    <Link to="/docs" activeOptions={{ exact: true }}>
      {item.title}
    </Link>
  )
}

function Entries({
  entries,
  pathname,
}: {
  entries: readonly DocsNavigationEntry[]
  pathname: string
}) {
  return (
    <ul>
      {entries.map((entry) => (
        <li key={entry.title}>
          {"items" in entry ? (
            <Collapsible defaultOpen={activeEntry(entry, pathname)}>
              <CollapsibleTrigger>
                <ChevronRight className="size-3 shrink-0" aria-hidden="true" />
                {entry.title}
              </CollapsibleTrigger>
              <CollapsibleContent>
                <Entries entries={entry.items} pathname={pathname} />
              </CollapsibleContent>
            </Collapsible>
          ) : (
            <DocsLink item={entry} />
          )}
        </li>
      ))}
    </ul>
  )
}

export function DocsSidebar() {
  const pathname = useLocation({
    select: (location) => withoutBasePath(location.pathname),
  })
  return (
    <nav className="docs-navigation" aria-label="Documentation sections">
      <h2 className="rail-heading">Documentation</h2>
      {docsNavigation.map((section) => (
        <Collapsible
          key={`${section.title}:${pathname}`}
          defaultOpen={section.items.some((entry) =>
            activeEntry(entry, pathname)
          )}
        >
          <CollapsibleTrigger>
            <ChevronRight className="size-3 shrink-0" aria-hidden="true" />
            {section.title}
          </CollapsibleTrigger>
          <CollapsibleContent>
            <Entries entries={section.items} pathname={pathname} />
          </CollapsibleContent>
        </Collapsible>
      ))}
      <div className="rail-note">
        <Link to="/">← Project overview</Link>
      </div>
    </nav>
  )
}
