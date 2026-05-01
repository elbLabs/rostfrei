import { ArrowUpRight } from "lucide-react"

export function SiteHeader() {
  return (
    <header className="border-b border-border/70">
      <div className="mx-auto flex max-w-[1180px] items-center justify-between px-5 py-5 sm:px-8">
        <a
          className="group inline-flex items-center gap-3"
          href="#top"
          aria-label="Rostfrei home"
        >
          <span className="grid size-8 place-items-center border border-primary/50 bg-primary/10 font-mono text-sm font-bold text-primary">
            R
          </span>
          <span className="text-sm font-semibold tracking-[0.18em] uppercase">
            Rostfrei
          </span>
        </a>

        <a
          className="inline-flex items-center gap-2 text-sm text-muted-foreground transition-colors hover:text-foreground"
          href="https://github.com/elbLabs/rostfrei"
        >
          Repository
          <ArrowUpRight aria-hidden="true" className="size-4" />
        </a>
      </div>
    </header>
  )
}
