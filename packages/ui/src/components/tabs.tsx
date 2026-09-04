import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Tabs as TabsPrimitive } from "radix-ui"

import { cn } from "../lib/utils"

function Tabs({ className, orientation = "horizontal", ...props }: React.ComponentProps<typeof TabsPrimitive.Root>) {
  return (
    <TabsPrimitive.Root
      data-slot="tabs"
      orientation={orientation}
      className={cn("group/tabs flex min-w-0 gap-3 data-[orientation=horizontal]:flex-col", className)}
      {...props}
    />
  )
}

const tabsListVariants = cva(
  "group/tabs-list inline-flex min-w-0 items-center text-muted-foreground group-data-[orientation=vertical]/tabs:flex-col group-data-[orientation=vertical]/tabs:items-stretch",
  {
    variants: {
      variant: {
        default: "w-fit border border-border bg-muted",
        line: "w-full justify-start border-b border-border group-data-[orientation=vertical]/tabs:w-fit group-data-[orientation=vertical]/tabs:border-r group-data-[orientation=vertical]/tabs:border-b-0",
      },
    },
    defaultVariants: { variant: "default" },
  }
)

function TabsList({ className, variant = "default", ...props }: React.ComponentProps<typeof TabsPrimitive.List> & VariantProps<typeof tabsListVariants>) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      data-variant={variant}
      className={cn(tabsListVariants({ variant }), className)}
      {...props}
    />
  )
}

function TabsTrigger({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      data-slot="tabs-trigger"
      className={cn(
        "inline-flex min-h-8 items-center justify-center gap-1.5 border-b-2 border-transparent px-3 py-1.5 text-xs font-medium whitespace-nowrap outline-none transition-colors hover:bg-accent/50 hover:text-foreground focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-ring/40 disabled:pointer-events-none disabled:opacity-50 data-[state=active]:border-primary data-[state=active]:bg-card data-[state=active]:text-primary group-data-[variant=line]/tabs-list:-mb-px group-data-[variant=line]/tabs-list:data-[state=active]:bg-transparent group-data-[orientation=vertical]/tabs:justify-start group-data-[orientation=vertical]/tabs:border-b-0 group-data-[orientation=vertical]/tabs:border-l-2 group-data-[orientation=vertical]/tabs:mb-0 [&_svg]:pointer-events-none [&_svg]:size-3.5 [&_svg]:shrink-0",
        className
      )}
      {...props}
    />
  )
}

function TabsContent({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Content>) {
  return (
    <TabsPrimitive.Content
      data-slot="tabs-content"
      className={cn("min-w-0 flex-1 text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-ring/40", className)}
      {...props}
    />
  )
}

export { Tabs, TabsList, TabsTrigger, TabsContent, tabsListVariants }
