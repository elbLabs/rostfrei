import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "../lib/utils"

const badgeVariants = cva(
  "inline-flex min-h-5 shrink-0 items-center gap-1 rounded-sm border px-1.5 py-0.5 font-mono text-[11px] leading-none whitespace-nowrap [&_svg]:size-3 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "border-primary/30 bg-accent text-accent-foreground",
        secondary: "border-border bg-secondary text-secondary-foreground",
        outline: "border-control-border bg-transparent text-foreground",
        neutral: "border-border bg-muted text-muted-foreground",
        success: "border-success/35 bg-success-soft text-success",
        warning: "border-warning/35 bg-warning-soft text-warning",
        info: "border-info/35 bg-info-soft text-info",
        destructive: "border-destructive/35 bg-destructive-soft text-destructive",
        danger: "border-destructive/35 bg-destructive-soft text-destructive",
        live: "border-info/35 bg-info-soft text-info",
        command: "border-command/35 bg-command-soft text-command",
        "domain-event": "border-domain-event/35 bg-domain-event-soft text-domain-event",
        "integration-event": "border-integration-event/35 bg-integration-event-soft text-integration-event",
      },
    },
    defaultVariants: { variant: "neutral" },
  }
)

type BadgeProps = React.ComponentProps<"span"> &
  VariantProps<typeof badgeVariants> & { asChild?: boolean }

function Badge({ className, variant = "neutral", asChild = false, ...props }: BadgeProps) {
  const Component = asChild ? Slot.Root : "span"
  return (
    <Component
      data-slot="badge"
      data-variant={variant}
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants, type BadgeProps }
