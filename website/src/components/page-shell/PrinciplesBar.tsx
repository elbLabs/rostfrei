import { Braces, History, Workflow } from "lucide-react"

export function PrinciplesBar() {
  return (
    <div className="grid gap-4 border-y border-border/70 py-5 text-sm text-muted-foreground md:grid-cols-3">
      <div className="flex items-center gap-3">
        <Braces aria-hidden="true" className="size-4 shrink-0 text-primary" />
        Business rules in ordinary Rust
      </div>
      <div className="flex items-center gap-3">
        <History aria-hidden="true" className="size-4 shrink-0 text-primary" />
        State rebuilt from domain events
      </div>
      <div className="flex items-center gap-3">
        <Workflow aria-hidden="true" className="size-4 shrink-0 text-primary" />
        Commands connected to their outcomes
      </div>
    </div>
  )
}
