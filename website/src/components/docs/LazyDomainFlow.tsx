import { Suspense, lazy } from "react"

const DomainFlow = lazy(() =>
  import("./DomainFlow").then((module) => ({
    default: module.DomainFlow,
  }))
)

export function LazyDomainFlow() {
  return (
    <Suspense
      fallback={
        <div className="my-8 grid h-96 place-items-center rounded-xl border border-border bg-muted/20 text-sm text-muted-foreground sm:h-105">
          Loading interactive flow…
        </div>
      }
    >
      <DomainFlow />
    </Suspense>
  )
}
