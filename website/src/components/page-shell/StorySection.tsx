import { Link } from "@tanstack/react-router"
import { ArrowRight } from "lucide-react"

const capabilities = [
  {
    title: "Find the rule",
    description:
      "Know where a business rule belongs, whoever wrote it. Checked project structure makes ownership visible, while typed declarations give humans and tools a navigable domain model.",
    link: "Explore project structure",
    slug: "project-structure",
  },
  {
    title: "Follow the change",
    description:
      "Understand a change through the business facts it records. Domain events preserve aggregate history; separate integration events describe what the application shares. NATS adapters provide durable storage and messaging.",
    link: "Follow the message flow",
    slug: "messaging/subjects",
  },
  {
    title: "Check the behavior",
    description:
      "Review an implementation against what it does. Discover commands through Tracer, use isolated Test execution and Studio to inspect the resulting message flow, and observe activity initiated by other clients.",
    link: "Try the bike-rental example",
    slug: "getting-started",
  },
]

export function StorySection() {
  return (
    <section aria-labelledby="story-title" className="content-section">
      <div className="max-w-3xl">
        <p className="section-kicker">For developers and coding agents</p>
        <h2 id="story-title" className="section-heading">
          Code gets written faster. Understanding has to keep up.
        </h2>
        <p className="mt-3 text-sm leading-6 text-muted-foreground">
          When an AI agent adds a feature, you still need to know where the rule
          lives, what it can change, and why it behaved that way. Rostfrei
          connects the domain model, event history, and runtime evidence so you
          can review the system in the language of the business.
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
        Humans can navigate the code and explore behavior in Studio. Coding
        agents can read the compiled model and discover command contracts
        through Tracer’s API. Both work from explicit contracts and observed
        outcomes.
      </p>
    </section>
  )
}

export function GettingStartedSection() {
  return (
    <section
      aria-labelledby="start-title"
      className="content-section grid gap-5 lg:grid-cols-2"
    >
      <div>
        <p className="section-kicker">Start with a real domain</p>
        <h2 id="start-title" className="section-heading">
          Read the model. Then rent a bicycle.
        </h2>
        <p className="mt-3 text-sm leading-6 text-muted-foreground">
          The bike-rental example connects fleet rules, accepted and rejected
          commands, event history, and public integration events. Inspect its
          compiled model first, then run the NATS-backed application and Studio.
        </p>
      </div>
      <div className="min-w-0 border border-border bg-muted p-4">
        <p className="text-sm font-medium">
          Inspect the model from a repository checkout
        </p>
        <pre className="mt-3 overflow-x-auto text-xs leading-6" tabIndex={0}>
          <code>
            cargo run --locked -p bike-rental \{"\n"} --bin bike-rental-model
          </code>
        </pre>
        <p className="mt-3 text-xs leading-5 text-muted-foreground">
          Uses the repository’s pinned Rust toolchain. No broker needed for this
          step.
        </p>
        <Link
          to="/docs/$"
          params={{ _splat: "getting-started" }}
          className="mt-4 inline-flex items-center gap-2 text-xs text-link"
        >
          Follow the getting-started guide{" "}
          <ArrowRight aria-hidden="true" className="size-4" />
        </Link>
      </div>
    </section>
  )
}
