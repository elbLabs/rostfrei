import { createFileRoute } from "@tanstack/react-router"

import {
  GettingStartedSection,
  HeroSection,
  RentalExampleSection,
  StorySection,
} from "@/components/page-shell"
import { SiteLayout } from "@/components/page-shell/SiteLayout"

export const Route = createFileRoute("/")({
  component: LandingPage,
})

function LandingPage() {
  return (
    <SiteLayout>
      <HeroSection />
      <GettingStartedSection />
      <StorySection />
      <RentalExampleSection />
    </SiteLayout>
  )
}
