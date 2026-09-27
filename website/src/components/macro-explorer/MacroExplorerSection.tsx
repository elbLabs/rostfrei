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
            Explicit behavior, discoverable contracts
          </p>
          <h2
            className="text-3xl font-semibold tracking-tight sm:text-5xl"
            id="macro-explorer-title"
          >
            Your Rust carries the rules. Macros describe the model.
          </h2>
          <p className="mt-5 max-w-2xl text-base leading-7 text-stone-400">
            See how domain declarations become metadata that the runtime and
            tooling can use. Each example pairs the Rust you write with a
            simplified view of the generated code.
          </p>
        </div>

        <MacroCarousel />
      </div>
    </section>
  )
}
