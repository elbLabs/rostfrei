import type { ReactNode } from "react"
import { SiteLayout } from "@/components/page-shell/SiteLayout"
import { DocsSidebar } from "./DocsSidebar"

export function DocsLayout({ children }: { children: ReactNode }) {
  return (
    <SiteLayout navigation={<DocsSidebar />} navigationTitle="Documentation">
      {children}
    </SiteLayout>
  )
}
