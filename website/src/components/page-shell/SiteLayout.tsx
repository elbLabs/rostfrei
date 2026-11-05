import { useState, type ReactNode } from "react"
import { Link } from "@tanstack/react-router"
import { Menu } from "lucide-react"
import { Button } from "@rostfrei/ui/components/button"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@rostfrei/ui/components/sheet"

import { SiteHeader } from "./SiteHeader"
import { SiteFooter } from "./SiteFooter"

function SiteNavigation() {
  return (
    <>
      <h2 className="rail-heading">Rostfrei</h2>
      <nav className="rail-links" aria-label="Project navigation">
        <Link to="/" activeOptions={{ exact: true }}>
          Overview
        </Link>
        <Link to="/docs/$" params={{ _splat: "getting-started" }}>
          Getting started
        </Link>
        <Link to="/docs/$" params={{ _splat: "project-structure" }}>
          Project structure
        </Link>
        <Link to="/docs/$" params={{ _splat: "domain-macros" }}>
          Domain macros
        </Link>
        <Link to="/docs/$" params={{ _splat: "messaging/subjects" }}>
          Subjects and streams
        </Link>
        <Link to="/docs/$" params={{ _splat: "read-models" }}>
          Read models
        </Link>
      </nav>
    </>
  )
}

export function SiteLayout({
  children,
  navigation,
  navigationTitle = "Project navigation",
}: {
  children: ReactNode
  navigation?: ReactNode
  navigationTitle?: string
}) {
  const [open, setOpen] = useState(false)
  const rail = navigation ?? <SiteNavigation />
  return (
    <div className="site-shell">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:block focus:p-3"
      >
        Skip to content
      </a>
      <SiteHeader />
      <div className="mobile-navigation">
        <Sheet open={open} onOpenChange={setOpen}>
          <SheetTrigger asChild>
            <Button variant="ghost" size="sm">
              <Menu aria-hidden="true" />
              {navigationTitle}
            </Button>
          </SheetTrigger>
          <SheetContent
            side="left"
            className="w-[min(300px,calc(100vw-32px))] overflow-y-auto"
          >
            <SheetHeader>
              <SheetTitle>{navigationTitle}</SheetTitle>
              <SheetDescription className="sr-only">
                Navigate the Rostfrei project and documentation.
              </SheetDescription>
            </SheetHeader>
            <div
              className="px-5 pb-6"
              onClick={(event) => {
                if ((event.target as HTMLElement).closest("a")) setOpen(false)
              }}
            >
              {rail}
            </div>
          </SheetContent>
        </Sheet>
      </div>
      <div className="site-body">
        <aside className="site-rail">{rail}</aside>
        <main id="main-content" className="page-content" tabIndex={-1}>
          {children}
        </main>
      </div>
      <SiteFooter />
    </div>
  )
}
