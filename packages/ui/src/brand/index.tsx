import { cn } from "../lib/utils"

// Statically analyzable URLs let Vite fingerprint assets and honor the app base.
const duckMark = new URL("./duck-mark.png", import.meta.url).href
const duckMarkDark = new URL("./duck-mark-dark.png", import.meta.url).href
const duck = new URL("./duck.png", import.meta.url).href
const duckDark = new URL("./duck-dark.png", import.meta.url).href

type BrandProps = { compact?: boolean; className?: string }
type DuckMascotProps = { className?: string; alt?: string }

/** Noninteractive wordmark. The containing link owns navigation and naming. */
function Brand({ compact = false, className }: BrandProps) {
  return (
    <span data-slot="brand" className={cn("inline-flex shrink-0 items-center gap-2 font-sans font-bold text-foreground", compact ? "text-xl tracking-[-0.7px]" : "text-[29px] tracking-[-1px]", className)}>
      <span aria-hidden="true" className={cn("inline-block shrink-0", compact ? "size-8" : "size-12")}>
        <img className="rf-duck-light size-full object-contain" src={duckMark} width={128} height={128} alt="" />
        <img className="rf-duck-dark size-full object-contain" src={duckMarkDark} width={128} height={128} alt="" />
      </span>
      <span>rostfrei</span>
    </span>
  )
}

function DuckMascot({ className, alt = "Rostfrei duck mascot" }: DuckMascotProps) {
  return (
    <span data-slot="duck-mascot" className={cn("inline-block w-42 max-w-full", className)}>
      <img className="rf-duck-light h-auto w-full" src={duck} width={512} height={469} alt={alt} />
      <img className="rf-duck-dark h-auto w-full" src={duckDark} width={512} height={469} alt={alt} />
    </span>
  )
}

export { Brand, DuckMascot, type BrandProps, type DuckMascotProps }
