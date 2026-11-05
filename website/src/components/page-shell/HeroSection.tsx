import { Link } from "@tanstack/react-router"
import { ArrowRight } from "lucide-react"
import { DuckMascot } from "@rostfrei/ui/brand"
import { Button } from "@rostfrei/ui/components/button"

const rentalCode = `#[domain_action(
    id = "rent-bicycle",
    label = "Rent bicycle"
)]
pub trait RentBicycleAction {
    fn rent_bicycle(
        &mut self,
        input: BicycleId,
    ) -> Result<(), BicycleUnavailable>;
}`

export function HeroSection() {
  return (
    <section className="min-w-0 pb-6" aria-labelledby="intro-title">
      <div className="hero-copy">
        <DuckMascot className="hero-duck" />
        <h1 id="intro-title" className="page-title">
          A batteries-included Rust framework for event-sourced applications.
        </h1>
        <p className="mt-4 text-sm leading-7">
          Domain code, durable commands, messaging, read models, and runtime
          inspection. Business rules stay in ordinary Rust.
        </p>
        <div className="mt-5 flex flex-wrap items-center gap-5">
          <Button asChild>
            <a href="#explore-with-your-agent">
              Explore with your agent <ArrowRight aria-hidden="true" />
            </a>
          </Button>
          <Link to="/docs" className="text-xs text-link underline">
            Read the docs
          </Link>
        </div>
      </div>
      <div
        className="hero-proof"
        aria-label="An accepted bicycle rental, from command to integration"
      >
        <div className="hero-proof-heading">
          <strong>One rental. A traceable flow.</strong>
          <span className="text-muted-foreground">bike-rental / fleet</span>
        </div>
        <div className="hero-proof-code">
          <p className="border-b border-border px-3 py-2 text-[10px] text-muted-foreground">
            rent_bicycle/action.rs · Rust declaration
          </p>
          <pre tabIndex={0} aria-label="Bicycle rental action declaration">
            <code>{rentalCode}</code>
          </pre>
        </div>
        <ol className="hero-proof-flow">
          <li>
            <span className="text-[11px] text-command">01 / Command</span>
            <br />
            <code>RentBicycle</code>
            <p>Ask the fleet to rent an available, serviceable bicycle.</p>
          </li>
          <li>
            <span className="text-[11px] text-domain-event">
              02 / Domain event
            </span>
            <br />
            <code>BicycleRented</code>
            <p>Commit the accepted change to the fleet’s private history.</p>
          </li>
          <li>
            <span className="text-[11px] text-integration-event">
              03 / Integration event
            </span>
            <br />
            <code>BicycleRentalStarted</code>
            <p>Publish a separate public contract after the commit.</p>
          </li>
        </ol>
        <p className="hero-proof-footer">
          Bicycle unavailable? The command is rejected and no domain events are
          appended.{" "}
          <Link
            to="/docs/$"
            params={{ _splat: "messaging/subjects" }}
            className="text-link"
          >
            Follow the message flow →
          </Link>
        </p>
      </div>
    </section>
  )
}
