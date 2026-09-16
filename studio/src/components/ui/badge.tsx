import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "inline-flex h-6 items-center gap-1 rounded-full border px-2 font-mono text-[12px] leading-none tracking-[0.08em] uppercase",
  {
    variants: {
      variant: {
        neutral: "border-white/12 bg-white/7 text-muted-foreground",
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
