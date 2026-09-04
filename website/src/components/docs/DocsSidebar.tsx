import { Link, useLocation } from "@tanstack/react-router"
import { BookOpenIcon, ChevronRightIcon } from "lucide-react"

import {
  type DocsNavigationEntry,
  type DocsNavigationGroup,
  type DocsNavigationItem,
  type DocsNavigationSection,
  docsNavigation,
} from "@/docs/navigation"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar"

function pathForSlug(slug: string) {
  return slug ? `/docs/${slug}` : "/docs"
}

function normalizePathname(pathname: string) {
  return pathname.length > 1 ? pathname.replace(/\/+$/, "") : pathname
}

function isNavigationGroup(
  entry: DocsNavigationEntry
): entry is DocsNavigationGroup {
  return "items" in entry
}

function itemIsActive(item: DocsNavigationItem, pathname: string) {
  return normalizePathname(pathForSlug(item.slug)) === pathname
}

function entryHasActiveItem(entry: DocsNavigationEntry, pathname: string) {
  return isNavigationGroup(entry)
    ? entry.items.some((item) => itemIsActive(item, pathname))
    : itemIsActive(entry, pathname)
}

function DocsLink({
  item,
  isActive,
}: {
  item: DocsNavigationItem
  isActive: boolean
}) {
  const { setOpenMobile } = useSidebar()
  const linkClassName =
    "data-status-active:bg-sidebar-accent data-status-active:font-medium data-status-active:text-sidebar-accent-foreground"

  if (item.slug === "") {
    return (
      <SidebarMenuSubButton asChild isActive={isActive}>
        <Link
          activeOptions={{ exact: true }}
          className={linkClassName}
          onClick={() => setOpenMobile(false)}
          to="/docs"
        >
          <span>{item.title}</span>
        </Link>
      </SidebarMenuSubButton>
    )
  }

  return (
    <SidebarMenuSubButton asChild isActive={isActive}>
      <Link
        activeOptions={{ exact: true }}
        className={linkClassName}
        onClick={() => setOpenMobile(false)}
        params={{ _splat: item.slug }}
        to="/docs/$"
      >
        <span>{item.title}</span>
      </Link>
    </SidebarMenuSubButton>
  )
}

function DocsNavSection({
  section,
  pathname,
}: {
  section: DocsNavigationSection
  pathname: string
}) {
  const hasActiveItem = section.items.some((entry) =>
    entryHasActiveItem(entry, pathname)
  )

  return (
    <Collapsible
      asChild
      className="group/collapsible"
      defaultOpen={hasActiveItem}
      key={pathname}
    >
      <SidebarMenuItem>
        <CollapsibleTrigger asChild>
          <SidebarMenuButton className="font-medium text-sidebar-foreground/80 data-[state=open]:text-sidebar-foreground">
            <ChevronRightIcon className="transition-transform duration-200 group-data-[state=open]/collapsible:rotate-90" />
            <span>{section.title}</span>
          </SidebarMenuButton>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <SidebarMenuSub>
            {section.items.map((entry) =>
              isNavigationGroup(entry) ? (
                <DocsNavGroup
                  group={entry}
                  key={entry.title}
                  pathname={pathname}
                />
              ) : (
                <SidebarMenuSubItem key={entry.slug || "index"}>
                  <DocsLink
                    item={entry}
                    isActive={itemIsActive(entry, pathname)}
                  />
                </SidebarMenuSubItem>
              )
            )}
          </SidebarMenuSub>
        </CollapsibleContent>
      </SidebarMenuItem>
    </Collapsible>
  )
}

function DocsNavGroup({
  group,
  pathname,
}: {
  group: DocsNavigationGroup
  pathname: string
}) {
  const hasActiveItem = group.items.some((item) => itemIsActive(item, pathname))

  return (
    <Collapsible
      asChild
      className="group/nav-group"
      defaultOpen={hasActiveItem}
      key={pathname}
    >
      <SidebarMenuSubItem>
        <CollapsibleTrigger className="flex h-7 w-full items-center gap-2 rounded-md px-2 text-sm text-sidebar-foreground/75 transition-colors outline-none hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring">
          <ChevronRightIcon className="size-3.5 transition-transform duration-200 group-data-[state=open]/nav-group:rotate-90" />
          <span>{group.title}</span>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <SidebarMenuSub className="mx-2.5 mr-0">
            {group.items.map((item) => (
              <SidebarMenuSubItem key={item.slug}>
                <DocsLink item={item} isActive={itemIsActive(item, pathname)} />
              </SidebarMenuSubItem>
            ))}
          </SidebarMenuSub>
        </CollapsibleContent>
      </SidebarMenuSubItem>
    </Collapsible>
  )
}

export function DocsSidebar() {
  const pathname = useLocation({
    select: (location) => normalizePathname(location.pathname),
  })

  return (
    <Sidebar
      aria-label="Documentation navigation"
      className="border-sidebar-border"
      collapsible="offcanvas"
    >
      <SidebarHeader className="h-[73px] shrink-0 justify-center border-b border-sidebar-border p-3">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild className="h-auto py-2" size="lg">
              <Link to="/docs">
                <span className="grid size-8 shrink-0 place-items-center border border-primary/45 bg-primary/10 text-primary">
                  <BookOpenIcon className="size-4" />
                </span>
                <span className="flex min-w-0 flex-col gap-0.5">
                  <span className="truncate font-semibold">Rostfrei</span>
                  <span className="truncate text-xs font-normal text-sidebar-foreground/60">
                    Documentation
                  </span>
                </span>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent className="py-3">
        <nav aria-label="Documentation sections">
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu>
                {docsNavigation.map((section) => (
                  <DocsNavSection
                    key={section.title}
                    pathname={pathname}
                    section={section}
                  />
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </nav>
      </SidebarContent>
      <SidebarRail />
    </Sidebar>
  )
}
