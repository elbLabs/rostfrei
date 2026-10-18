import { createFileRoute } from "@tanstack/react-router"

import { MacroExplorerSection } from "@/components/macro-explorer/MacroExplorerSection"
import {
  GettingStartedSection,
  HeroSection,
  PrinciplesBar,
  StorySection,
} from "@/components/page-shell"
import { ProjectStructureSection } from "@/components/project-structure/ProjectStructureSection"
import { SiteLayout } from "@/components/page-shell/SiteLayout"

export const Route = createFileRoute("/")({
  component: LandingPage,
})

function LandingPage() {
  return (
    <SiteLayout>
      <HeroSection />
      <PrinciplesBar />
      <StorySection />
      <ProjectStructureSection />
      <MacroExplorerSection />
      <GettingStartedSection />
    </SiteLayout>
  )
}
