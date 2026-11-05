import { Link } from "@tanstack/react-router"
import { ArrowRight } from "lucide-react"
import { AgentPrompt } from "@/components/AgentPrompt"

const capabilities = [
  {
    title: "Domain code",
    description:
      "Actions, policies, and event application are ordinary Rust. Domain declarations attach names and ownership metadata. Aggregates stay independent of persistence and messaging adapters.",
    link: "Domain declarations",
    slug: "domain-macros",
  },
  {
    title: "Command runtime",
    description:
      "Load history, check stream versions, and atomically commit accepted events and completed outcomes. Transactions can span aggregates in one store. Exact retries return the recorded decision.",
    link: "Command execution",
    slug: "command-runtime",
  },
  {
    title: "Messaging and HTTP",
    description:
      "Typed buses connect commands, queries, and integration events. NATS JetStream supplies durable storage and delivery; the HTTP adapter exposes routes the application mounts explicitly.",
    link: "Subjects and streams",
    slug: "messaging/subjects",
  },
  {
    title: "Read models",
    description:
      "Typed event handlers maintain saved query views. The runtime persists each value with its source checkpoints and retries concurrent updates. Queries use a read-only handle.",
    link: "Declare and query",
    slug: "read-models",
  },
  {
    title: "Testing and inspection",
    description:
      "Discover command contracts, run behavioral tests against isolated application resources, and inspect observed message flows. Tracer exposes the API; coding agents and Studio can use it.",
    link: "Explore the example",
    slug: "getting-started",
  },
  {
    title: "Project tooling",
    description:
      "Domain declarations and implementations follow a checked directory structure. cargo rostfrei check validates placement, ownership, and implementation targets. Compiled domain metadata is available to tools.",
    link: "Structure and checks",
    slug: "project-structure",
  },
]

export function StorySection() {
  return (
    <section aria-labelledby="story-title" className="content-section">
      <div className="max-w-3xl">
        <p className="section-kicker">Batteries included</p>
        <h2 id="story-title" className="section-heading">
          The pieces around your domain code.
        </h2>
        <p className="mt-3 text-sm leading-6 text-muted-foreground">
          Applications depend on <code>rostfrei</code> and enable the adapters
          they need. The framework covers execution, storage, communication,
          queries, and tools for working with a running application.
        </p>
      </div>
      <div className="mt-5 grid gap-5 lg:grid-cols-3">
        {capabilities.map((capability, index) => (
          <div
            key={capability.title}
            className="flex min-w-0 flex-col border-t border-border pt-3"
          >
            <span className="font-mono text-xs text-primary">0{index + 1}</span>
            <h3 className="mt-1 text-sm font-semibold">{capability.title}</h3>
            <p className="mt-2 flex-1 text-xs leading-6 text-muted-foreground">
              {capability.description}
            </p>
            <Link
              to="/docs/$"
              params={{ _splat: capability.slug }}
              className="mt-3 inline-flex items-center gap-2 text-xs text-link"
            >
              {capability.link}{" "}
              <ArrowRight aria-hidden="true" className="size-4" />
            </Link>
          </div>
        ))}
      </div>
      <p className="mt-5 border-l-2 border-primary pl-4 text-xs leading-6 text-muted-foreground">
        A bicycle was added, rented, then returned. Those recorded facts
        reconstruct its availability and support new query views. The application
        still decides which bicycles are eligible and whether a return needs an
        inspection.{" "}
        <Link
          to="/docs"
          hash="why-events-are-the-foundation"
          className="text-link"
        >
          Read the execution model and tradeoffs →
        </Link>
      </p>
    </section>
  )
}

export function GettingStartedSection() {
  return (
    <section
      id="explore-with-your-agent"
      aria-labelledby="start-title"
      className="content-section"
    >
      <div>
        <p className="section-kicker">Start with a real application</p>
        <h2 id="start-title" className="section-heading">
          Explore with your agent
        </h2>
        <p className="mt-3 text-sm leading-6 text-muted-foreground">
          Give your coding agent this prompt. It will check your environment,
          prepare an isolated bike-rental demo, and guide you through real
          outcomes one step at a time. You choose what to inspect next.
        </p>
      </div>
      <AgentPrompt />
      <Link
        to="/docs/$"
        params={{ _splat: "getting-started" }}
        className="mt-4 inline-flex items-center gap-2 text-xs text-link"
      >
        Prerequisites and reference links{" "}
        <ArrowRight aria-hidden="true" className="size-4" />
      </Link>
    </section>
  )
}
