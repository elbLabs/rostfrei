import { MacroExplorerSection } from "@/components/macro-explorer/MacroExplorerSection"
import {
  HeroSection,
  PrinciplesBar,
  SiteFooter,
  SiteHeader,
} from "@/components/page-shell"
import { ProjectStructureSection } from "@/components/project-structure/ProjectStructureSection"

export function App() {
  return (
    <div className="min-h-svh overflow-hidden bg-background text-foreground">
      <SiteHeader />
      <main
        id="top"
        className="mx-auto w-full max-w-[1180px] min-w-0 overflow-hidden px-5 sm:px-8"
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

export default App
