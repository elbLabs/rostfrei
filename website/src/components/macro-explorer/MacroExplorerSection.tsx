import { MacroCarousel } from "./MacroCarousel"

export function MacroExplorerSection() {
  return (
    <section
      aria-labelledby="macro-explorer-title"
      className="w-full max-w-full min-w-0 overflow-hidden bg-[#17130f] px-4 py-16 text-stone-100 sm:px-6 lg:px-8 lg:py-24"
    >
      <div className="mx-auto max-w-7xl">
        <div className="mb-10 max-w-3xl">
          <p className="mb-3 font-mono text-xs font-semibold tracking-[0.18em] text-primary uppercase">
            Macro by macro
          </p>
          <h2
            className="text-3xl font-semibold tracking-tight sm:text-5xl"
            id="macro-explorer-title"
          >
            Small markers. Visible Rust.
          </h2>
          <p className="mt-5 max-w-2xl text-base leading-7 text-stone-400">
            Move through the complete public macro surface. Each slide pairs the
            authored domain code with a simplified view of what Rostfrei
            generates.
          </p>
        </div>

        <MacroCarousel />
      </div>
    </section>
  )
}
