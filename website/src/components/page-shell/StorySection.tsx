import { Link } from "@tanstack/react-router"
import { AgentPrompt } from "@/components/AgentPrompt"

const capabilities = [
  {
    title: "Domain model",
    description:
      "Actions, policies, and event application stay in ordinary Rust. Declarations describe their roles and ownership, with a checked project structure that helps you find each implementation.",
    slug: "domain-macros",
  },
  {
    title: "Command execution",
    description:
      "The runtime loads history, checks stream versions, and atomically commits accepted events and completed outcomes. Transactions can span aggregates in one store; exact retries return the recorded decision.",
    slug: "command-runtime",
  },
  {
    title: "Messaging and HTTP",
    description:
      "Typed buses connect commands, queries, and integration events. NATS JetStream provides durable storage and delivery, while the HTTP adapter exposes routes you mount explicitly.",
    slug: "messaging/subjects",
  },
  {
    title: "Read models",
    description:
      "Event handlers maintain saved views for queries. The runtime persists each value with its source checkpoints and retries concurrent updates; your application defines the view and how it evolves.",
    slug: "read-models",
  },
]

export function StorySection() {
  return (
    <section aria-labelledby="framework-title" className="content-section">
      <h2 id="framework-title" className="section-heading">
        How the framework fits together
      </h2>
      <p className="section-intro">
        Use <code>rostfrei</code> as your framework dependency and enable the
        adapters your application needs.
      </p>
      <dl className="framework-list">
        {capabilities.map((capability) => (
          <div key={capability.title}>
            <dt>
              <Link to="/docs/$" params={{ _splat: capability.slug }}>
                {capability.title}
              </Link>
            </dt>
            <dd>{capability.description}</dd>
          </div>
        ))}
      </dl>
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
      <h2 id="start-title" className="section-heading">
        Explore with your agent
      </h2>
      <p className="section-intro">
        Copy this prompt to run the bike-rental example and inspect a successful
        rental and a rejection, one step at a time.
      </p>
      <AgentPrompt />
      <Link
        to="/docs/$"
        params={{ _splat: "getting-started" }}
        className="section-link text-link"
      >
        Setup and prerequisites →
      </Link>
    </section>
  )
}
