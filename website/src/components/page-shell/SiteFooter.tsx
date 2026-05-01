export function SiteFooter() {
  return (
    <footer className="mt-24 border-t border-border/70">
      <div className="mx-auto flex max-w-[1180px] flex-col gap-2 px-5 py-8 text-xs text-muted-foreground sm:flex-row sm:items-center sm:justify-between sm:px-8">
        <p>Rostfrei · explicit domains in ordinary Rust.</p>
        <code>cargo rostfrei check --workspace</code>
      </div>
    </footer>
  )
}
