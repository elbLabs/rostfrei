import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "inline-flex h-5 items-center gap-1 rounded-full border px-1.5 font-mono text-[9px] leading-none tracking-[0.08em] uppercase",
  {
    variants: {
      variant: {
        neutral: "border-white/8 bg-white/5 text-muted-foreground",
        success: "border-emerald-300/15 bg-emerald-300/8 text-emerald-300",
        danger: "border-red-300/15 bg-red-300/8 text-red-300",
        live: "border-cyan-300/15 bg-cyan-300/8 text-cyan-200",
      },
    },
    defaultVariants: {
      variant: "neutral",
    },
  }
)

function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return (
    <span
      data-slot="badge"
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  )
}

export { Badge }
