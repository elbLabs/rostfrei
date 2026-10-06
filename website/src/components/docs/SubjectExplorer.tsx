import { useId, useState } from "react"

import { Button } from "@/components/ui/button"
import { subjectFamilies } from "@/docs/subject-data"

export function SubjectExplorer() {
  const labelId = useId()
  const [scope, setScope] = useState<"normal" | "test">("normal")
  const isTest = scope === "test"
  const subjectPrefix = isTest ? "bike-rental.test" : "bike-rental"
  const streamPrefix = isTest ? "BIKE_RENTAL__TEST" : "BIKE_RENTAL"
  const tokens = [
    { label: "Application", value: "bike-rental" },
    ...(isTest ? [{ label: "Traffic scope", value: "test" }] : []),
    { label: "Kind", value: "command" },
    { label: "Bounded context", value: "bike-rental" },
    { label: "Business name", value: "rent-bicycle" },
  ]

  return (
    <section
      aria-label="Bike-rental subject explorer"
      className="my-6 min-w-0 rounded-xl border border-border bg-card p-4 sm:p-6"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <span className="font-medium text-foreground" id={labelId}>
          Traffic scope
        </span>
        <div aria-labelledby={labelId} className="flex gap-2" role="group">
          {(["normal", "test"] as const).map((value) => (
            <Button
              aria-pressed={scope === value}
              key={value}
              onClick={() => setScope(value)}
              type="button"
              variant={scope === value ? "default" : "outline"}
            >
              {value === "test" ? "Test" : "Normal"}
            </Button>
          ))}
        </div>
      </div>
      <p className="mt-3 text-sm leading-6 text-muted-foreground" role="status">
        {isTest
          ? "Test inserts .test in subjects and __TEST in stream names. The application identity is still bike-rental."
          : "Normal traffic uses the canonical namespace, with no extra scope token."}
      </p>

      <dl className="my-5 flex flex-wrap gap-2">
        {tokens.map((token) => (
          <div
            className="rounded-lg border border-primary/25 bg-primary/5 px-3 py-2"
            key={token.label}
          >
            <dt className="text-xs text-muted-foreground">{token.label}</dt>
            <dd className="mt-1 font-mono text-sm text-foreground">
              {token.value}
            </dd>
          </div>
        ))}
      </dl>

      <div className="grid gap-3 xl:grid-cols-2">
        {subjectFamilies.map((family) => (
          <article
            className="min-w-0 rounded-lg border border-border bg-background p-4"
            key={family.title}
          >
            <h3 className="font-semibold text-foreground">{family.title}</h3>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              {family.description}
            </p>
            <dl className="mt-4 space-y-3 text-xs">
              <div>
                <dt className="text-muted-foreground">Subject</dt>
                <dd className="mt-1">
                  <code className="font-mono leading-6 wrap-anywhere text-foreground">
                    {subjectPrefix}.{family.subject}
                  </code>
                </dd>
              </div>
              {family.filters.length > 0 ? (
                <div>
                  <dt className="text-muted-foreground">Stream filters</dt>
                  <dd className="mt-1 space-y-1">
                    {family.filters.map((filter) => (
                      <code
                        className="block font-mono leading-6 wrap-anywhere text-foreground"
                        key={filter}
                      >
                        {subjectPrefix}.{filter}
                      </code>
                    ))}
                  </dd>
                </div>
              ) : null}
              <div>
                <dt className="text-muted-foreground">JetStream stream</dt>
                <dd className="mt-1">
                  {family.stream ? (
                    <code className="font-mono leading-6 wrap-anywhere text-foreground">
                      {streamPrefix}
                      {family.stream}
                    </code>
                  ) : (
                    <span className="text-foreground">
                      None — Core NATS request/reply
                    </span>
                  )}
                </dd>
              </div>
            </dl>
          </article>
        ))}
      </div>
    </section>
  )
}
