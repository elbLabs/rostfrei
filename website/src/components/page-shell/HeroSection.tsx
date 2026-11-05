import { Link } from "@tanstack/react-router"
import { DuckMascot } from "@rostfrei/ui/brand"

export function HeroSection() {
  return (
    <section className="hero" aria-labelledby="intro-title">
      <div className="hero-heading">
        <h1 id="intro-title" className="page-title">
          A batteries-included Rust framework for{" "}
          <span className="whitespace-nowrap">event-sourced</span> applications.
        </h1>
        <DuckMascot className="hero-duck" alt="" />
      </div>
      <p className="hero-description">
        rostfrei connects your domain model to durable commands, messaging, and
        read models. You write the business rules in Rust and inspect their
        outcomes through Tracer.
      </p>
      <div className="hero-actions">
        <Link to="/docs" className="text-link">
          Read the documentation
        </Link>
      </div>
    </section>
  )
}
