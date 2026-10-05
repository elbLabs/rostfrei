import { Link } from "@tanstack/react-router"
import { ArrowRight } from "lucide-react"

import { Button } from "@/components/ui/button"

export function HeroSection() {
  return (
    <section className="grid min-w-0 gap-12 py-16 md:py-24 lg:grid-cols-[1.2fr_0.8fr] lg:items-center">
      <div className="min-w-0">
        <p className="mb-5 font-mono text-xs font-semibold tracking-[0.18em] text-primary uppercase">
          Domain-driven, event-sourced Rust
        </p>
        <h1 className="max-w-full text-5xl leading-[1.05] font-semibold tracking-[-0.055em] text-balance sm:text-6xl xl:text-7xl">
          Understand your code.
          <span className="text-muted-foreground"> Even when AI wrote it.</span>
        </h1>
        <p className="mt-6 max-w-xl text-base leading-7 text-muted-foreground sm:text-lg">
          Rostfrei is a Rust framework for domain modeling, event sourcing, and
          messaging. Give humans and AI agents a shared model of the business,
          with explicit ownership, event history, and behavior you can inspect.
        </p>
        <div className="mt-8 flex flex-wrap gap-3">
          <Button asChild className="h-11 px-5">
            <Link to="/docs/$" params={{ _splat: "getting-started" }}>
              Run the example <ArrowRight aria-hidden="true" />
            </Link>
          </Button>
          <Button asChild variant="outline" className="h-11 px-5">
            <Link to="/docs">Explore the docs</Link>
          </Button>
        </div>
        <p className="mt-5 text-xs leading-5 text-muted-foreground">
          Open source · EUPL-1.2 · Alpha — APIs are evolving
        </p>
      </div>

      <aside
        aria-label="An accepted bicycle rental, from command to integration"
        className="min-w-0 rounded-2xl border border-border bg-card p-6 sm:p-8"
      >
        <p className="font-mono text-xs tracking-[0.14em] text-muted-foreground uppercase">
          One rental. A traceable flow.
        </p>
        <ol className="mt-7 space-y-6 border-l border-primary/30 pl-5">
          <li>
            <p className="text-xs font-medium text-primary">01 / Command</p>
            <p className="mt-1 font-mono text-sm break-words">RentBicycle</p>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              Ask the fleet to rent an available, serviceable bicycle.
            </p>
          </li>
          <li>
            <p className="text-xs font-medium text-primary">
              02 / Domain event
            </p>
            <p className="mt-1 font-mono text-sm break-words">BicycleRented</p>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              Commit the accepted change to the fleet’s private history.
            </p>
          </li>
          <li>
            <p className="text-xs font-medium text-primary">
              03 / Integration event
            </p>
            <p className="mt-1 font-mono text-sm break-words">
              BicycleRentalStarted
            </p>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              Publish a separate public contract after the commit.
            </p>
          </li>
        </ol>
        <p className="mt-7 border-t border-border pt-4 text-xs leading-5 text-muted-foreground">
          Bicycle unavailable? The command is rejected and no domain events are
          appended.
        </p>
      </aside>
    </section>
  )
}
