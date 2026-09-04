import * as React from "react"
import { ChevronDownIcon } from "lucide-react"

import { cn } from "../lib/utils"

type NativeSelectProps = React.ComponentProps<"select"> & {
  wrapperClassName?: string
}

function NativeSelect({ className, wrapperClassName, children, multiple, size, ...props }: NativeSelectProps) {
  const isList = multiple || (size !== undefined && size > 1)

  return (
    <div data-slot="native-select-wrapper" className={cn("relative w-full min-w-0", wrapperClassName)}>
      <select
        data-slot="native-select"
        multiple={multiple}
        size={size}
        className={cn(
          "w-full min-w-0 appearance-none rounded-md border border-input bg-card px-2.5 py-1 text-[13px] text-foreground outline-none transition-colors focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:bg-muted disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive/25",
          isList ? "py-2" : "h-8 pr-8",
          className
        )}
        {...props}
      >
        {children}
      </select>
      {!isList && <ChevronDownIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 right-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />}
    </div>
  )
}

function NativeSelectOption(props: React.ComponentProps<"option">) {
  return <option data-slot="native-select-option" {...props} />
}

function NativeSelectOptGroup(props: React.ComponentProps<"optgroup">) {
  return <optgroup data-slot="native-select-optgroup" {...props} />
}

export { NativeSelect, NativeSelectOption, NativeSelectOptGroup, type NativeSelectProps }
