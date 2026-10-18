import { MacroCarousel } from "./MacroCarousel"

export function MacroExplorerSection() {
  return (
    <section aria-labelledby="macro-explorer-title" className="content-section">
      <div className="mx-auto max-w-7xl">
        <div className="mb-5">
          <p className="section-kicker">Domain macros</p>
          <h2 className="section-heading" id="macro-explorer-title">
            What you write. What Rostfrei generates.
          </h2>
          <p className="mt-3 text-sm leading-6 text-muted-foreground">
            Browse domain declarations and behavior attributes. Each example
            pairs application code with a simplified expansion.
          </p>
        </div>

        <MacroCarousel />
      </div>
    </section>
  )
}
