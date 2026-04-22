import type * as React from "react";
import { cn } from "../../lib/utils";

function Label({ className, ...props }: React.ComponentProps<"label">) {
  // biome-ignore lint/a11y/noLabelWithoutControl: Call sites supply htmlFor, nest a control, or use this visual label as a section heading.
  return <label className={cn("ui-label", className)} {...props} />;
}

export { Label };
