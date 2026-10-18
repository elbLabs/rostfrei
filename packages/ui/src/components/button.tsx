import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"

import { cn } from "../lib/utils"

const buttonVariants = cva(
  "group/button inline-flex shrink-0 items-center justify-center rounded-md border border-transparent text-[13px] font-medium whitespace-nowrap transition-colors outline-none select-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40 disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/25 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        default: "border-primary bg-primary text-primary-foreground hover:bg-primary/90",
        outline: "border-control-border bg-card text-foreground hover:bg-muted aria-expanded:bg-accent aria-expanded:text-accent-foreground",
        secondary: "border-border bg-secondary text-secondary-foreground hover:border-control-border hover:bg-accent hover:text-accent-foreground",
        ghost: "text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-accent aria-expanded:text-accent-foreground",
        destructive: "border-destructive/40 bg-destructive-soft text-destructive hover:bg-destructive hover:text-destructive-foreground focus-visible:ring-destructive/40",
        link: "text-link underline-offset-4 hover:underline",
      },
      size: {
        default: "h-8 gap-1.5 px-3",
        xs: "h-6 gap-1 px-2 text-xs [&_svg:not([class*='size-'])]:size-3",
        sm: "h-7 gap-1 px-2.5 text-xs",
        lg: "h-9 gap-2 px-4",
        icon: "size-8",
        "icon-xs": "size-6 [&_svg:not([class*='size-'])]:size-3",
        "icon-sm": "size-7",
        "icon-lg": "size-9 [&_svg:not([class*='size-'])]:size-4",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  }
)

type ButtonProps = React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & { asChild?: boolean }

function Button({
  className,
  variant = "default",
  size = "default",
  asChild = false,
  ...props
}: ButtonProps) {
  const Component = asChild ? Slot.Root : "button"

  return (
    <Component
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  )
}

export { Button, buttonVariants, type ButtonProps }
