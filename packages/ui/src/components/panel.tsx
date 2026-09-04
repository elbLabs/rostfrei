import * as React from "react"

import { cn } from "../lib/utils"

function Panel({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="panel" className={cn("min-w-0 border border-border bg-card text-card-foreground", className)} {...props} />
}

function PanelHeader({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="panel-header" className={cn("flex min-h-9 flex-wrap items-center justify-between gap-2 border-b border-border bg-muted px-3 py-2", className)} {...props} />
}

function PanelTitle({ className, ...props }: React.ComponentProps<"h2">) {
  return <h2 data-slot="panel-title" className={cn("text-xs leading-snug font-semibold", className)} {...props} />
}

function PanelContent({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="panel-content" className={cn("min-w-0 p-3 text-[13px]", className)} {...props} />
}

export { Panel, PanelHeader, PanelTitle, PanelContent }
