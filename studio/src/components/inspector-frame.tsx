import { X } from "lucide-react"
import type { ComponentPropsWithoutRef, ReactNode } from "react"

import { Button } from "@/components/ui/button"

interface InspectorFrameProps extends ComponentPropsWithoutRef<"aside"> {
  as?: "aside" | "section"
  label: string
  back?: ReactNode
  headingIcon?: ReactNode
  close?: { label: string; onClick: () => void }
}

export function InspectorFrame({
  as: Element = "aside",
  label,
  className,
  back,
  headingIcon,
  close,
  children,
  ...props
}: InspectorFrameProps) {
  return (
    <Element
      {...props}
      className={className ? `inspector-frame ${className}` : "inspector-frame"}
      aria-label={label}
    >
      <div className="inspector-heading">
        {back}
        <span>{label}</span>
        {close ? (
          <Button
            className="inspector-close"
            variant="ghost"
            size="icon"
            onClick={close.onClick}
            aria-label={close.label}
          >
            <X aria-hidden="true" />
          </Button>
        ) : (
          headingIcon
        )}
      </div>
      {children}
    </Element>
  )
}
