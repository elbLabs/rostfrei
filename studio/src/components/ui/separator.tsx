import { cn } from "@/lib/utils"

function Separator({ className }: { className?: string }) {
  return (
    <div
      data-slot="separator"
      role="separator"
      className={cn("h-px w-full bg-white/7", className)}
    />
  )
}

export { Separator }
