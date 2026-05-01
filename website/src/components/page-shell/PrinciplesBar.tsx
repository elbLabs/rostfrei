import { Braces, FolderTree } from "lucide-react"

export function PrinciplesBar() {
  return (
    <div className="mb-10 grid gap-3 border-y border-border/70 py-4 text-sm text-muted-foreground sm:grid-cols-2">
      <div className="flex items-center gap-3">
        <FolderTree aria-hidden="true" className="size-4 text-primary" />
        Structure communicates ownership
      </div>
      <div className="flex items-center gap-3">
        <Braces aria-hidden="true" className="size-4 text-primary" />
        Traits keep behavior explicit
      </div>
    </div>
  )
}
