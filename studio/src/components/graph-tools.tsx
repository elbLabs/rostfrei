import type { ReactNode } from "react"
import {
  Background,
  BackgroundVariant,
  useReactFlow,
  useViewport,
} from "@xyflow/react"
import { Focus, Minus, Plus } from "lucide-react"
import { Button } from "@/components/ui/button"

export function GraphBackground() {
  return (
    <Background
      variant={BackgroundVariant.Dots}
      gap={20}
      size={1.2}
      color="var(--canvas-dot)"
    />
  )
}

export function GraphHeading({
  title,
  icon,
  children,
}: {
  title: string
  icon: ReactNode
  children: ReactNode
}) {
  return (
    <header className="flow-heading">
      <div>
        {icon}
        <h2>{title}</h2>
      </div>
      {children}
    </header>
  )
}

export function GraphControls({
  onFit,
  fitLabel = "Fit graph to view",
}: {
  onFit: () => void
  fitLabel?: string
}) {
  const { zoomIn, zoomOut } = useReactFlow()
  const { zoom } = useViewport()
  return (
    <div
      className="graph-controls nodrag nopan nowheel"
      role="group"
      aria-label="Canvas controls"
    >
      <Button
        variant="ghost"
        size="icon"
        onClick={() => void zoomOut({ duration: 150 })}
        aria-label="Zoom out"
      >
        <Minus />
      </Button>
      <span className="graph-zoom-value">{Math.round(zoom * 100)}%</span>
      <Button
        variant="ghost"
        size="icon"
        onClick={() => void zoomIn({ duration: 150 })}
        aria-label="Zoom in"
      >
        <Plus />
      </Button>
      <Button variant="ghost" size="icon" onClick={onFit} aria-label={fitLabel}>
        <Focus />
      </Button>
    </div>
  )
}
