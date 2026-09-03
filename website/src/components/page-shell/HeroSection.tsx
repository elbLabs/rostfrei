export function HeroSection() {
  return (
    <section className="grid min-w-0 gap-10 py-20 md:grid-cols-[1fr_0.72fr] md:items-end md:py-28">
      <div className="min-w-0">
        <p className="mb-5 font-mono text-xs font-semibold tracking-[0.18em] text-primary uppercase">
          Typed domain structure for Rust
        </p>
        <h1 className="max-w-full text-5xl leading-[0.98] font-semibold tracking-[-0.055em] text-balance sm:text-7xl">
          One domain concept,
          <span className="text-muted-foreground"> a few small files.</span>
        </h1>
      </div>
      <p className="max-w-xl text-base leading-7 text-muted-foreground md:pb-1 md:text-lg">
        Rostfrei keeps behavior in ordinary Rust and uses project structure to
        make ownership visible. Thin macros add semantic metadata; the checker
        makes the filesystem part of the contract.
      </p>
    </section>
  )
}
