import type { ReactNode } from "react"

import { SiteHeader } from "@/components/page-shell"
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar"

import { DocsSidebar } from "./DocsSidebar"

export function DocsLayout({ children }: { children: ReactNode }) {
  return (
    <SidebarProvider>
      <DocsSidebar />
      <SidebarInset className="min-h-svh min-w-0">
        <div className="sticky top-0 z-30 flex h-[73px] shrink-0 border-b border-border/70 bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/85">
          <div className="flex items-center px-2 sm:px-3">
            <SidebarTrigger className="text-muted-foreground hover:text-foreground" />
          </div>
          <div className="min-w-0 flex-1 [&>header]:border-0">
            <SiteHeader />
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-col">{children}</div>
      </SidebarInset>
    </SidebarProvider>
  )
}
