import { createFileRoute } from "@tanstack/react-router"

import { MacroExplorerSection } from "@/components/macro-explorer/MacroExplorerSection"
import {
  HeroSection,
  PrinciplesBar,
  SiteFooter,
  SiteHeader,
} from "@/components/page-shell"
import { ProjectStructureSection } from "@/components/project-structure/ProjectStructureSection"

export const Route = createFileRoute("/")({
  component: LandingPage,
})

function LandingPage() {
  return (
    <div className="flex min-h-svh flex-col overflow-hidden">
      <SiteHeader />
      <main
        id="top"
        className="mx-auto w-full max-w-295 min-w-0 flex-1 overflow-hidden px-5 sm:px-8"
      >
        <HeroSection />
        <PrinciplesBar />
        <ProjectStructureSection />
        <MacroExplorerSection />
      </main>
      <SiteFooter />
    </div>
  )
}
