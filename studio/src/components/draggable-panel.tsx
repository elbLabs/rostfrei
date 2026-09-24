import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react"
import { GripHorizontal, X, type LucideIcon } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface Point {
  x: number
  y: number
}
interface DraggablePanelProps {
  id: string
  className: string
  label: string
  icon: LucideIcon
  open: boolean
  initialPosition: Point
  closeLabel: string
  onClose: () => void
  children: ReactNode
}
let nextPanelLayer = 80

export function DraggablePanel({
  id,
  className,
  label,
  icon: Icon,
  open,
  initialPosition,
  closeLabel,
  onClose,
  children,
}: DraggablePanelProps) {
  const panelRef = useRef<HTMLElement>(null)
  const drag = useRef<{
    pointerId: number
    offsetX: number
    offsetY: number
    position: Point
  } | null>(null)
  const [position, setPosition] = useState(initialPosition)
  useEffect(() => {
    const panel = panelRef.current
    const keepOnScreen = () =>
      setPosition((current) => clampPanelPosition(panel, current))
    keepOnScreen()
    window.addEventListener("resize", keepOnScreen)
    return () => {
      window.removeEventListener("resize", keepOnScreen)
      document.body.classList.remove("is-dragging-panel")
    }
  }, [])
  useEffect(() => {
    if (open && panelRef.current)
      panelRef.current.style.zIndex = String(++nextPanelLayer)
  }, [open])
  const finishDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return
    setPosition(drag.current.position)
    drag.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId))
      event.currentTarget.releasePointerCapture(event.pointerId)
    panelRef.current?.classList.remove("studio-panel-dragging")
    document.body.classList.remove("is-dragging-panel")
  }
  const style = { left: position.x, top: position.y } satisfies CSSProperties
  return (
    <aside
      ref={panelRef}
      id={id}
      className={cn(
        "studio-floating-panel",
        className,
        open && "studio-floating-panel-open"
      )}
      style={style}
      aria-label={label}
      aria-hidden={!open}
      inert={!open}
      onKeyDown={(event) => {
        if (event.key === "Escape") onClose()
      }}
      onPointerDownCapture={() => {
        if (panelRef.current)
          panelRef.current.style.zIndex = String(++nextPanelLayer)
      }}
    >
      <div className="flex h-full min-h-0 flex-col">
        <div
          className="studio-panel-header"
          onPointerDown={(event) => {
            if (
              event.button !== 0 ||
              (event.target instanceof Element &&
                event.target.closest("button"))
            )
              return
            const bounds = panelRef.current?.getBoundingClientRect()
            if (!bounds) return
            drag.current = {
              pointerId: event.pointerId,
              offsetX: event.clientX - bounds.left,
              offsetY: event.clientY - bounds.top,
              position,
            }
            event.currentTarget.setPointerCapture(event.pointerId)
            panelRef.current?.classList.add("studio-panel-dragging")
            document.body.classList.add("is-dragging-panel")
          }}
          onPointerMove={(event) => {
            if (drag.current?.pointerId !== event.pointerId) return
            const next = clampPanelPosition(panelRef.current, {
              x: event.clientX - drag.current.offsetX,
              y: event.clientY - drag.current.offsetY,
            })
            drag.current.position = next
            if (panelRef.current) {
              panelRef.current.style.left = `${next.x}px`
              panelRef.current.style.top = `${next.y}px`
            }
          }}
          onPointerUp={finishDrag}
          onPointerCancel={finishDrag}
        >
          <span>
            <Icon size={15} />
            {label}
          </span>
          <GripHorizontal
            className="studio-panel-grip"
            size={16}
            aria-hidden="true"
          />
          <Button
            variant="ghost"
            size="icon"
            onClick={onClose}
            aria-label={closeLabel}
          >
            <X />
          </Button>
        </div>
        {children}
      </div>
    </aside>
  )
}

function clampPanelPosition(panel: HTMLElement | null, next: Point): Point {
  if (!panel) return next
  const bounds = panel.getBoundingClientRect()
  return {
    x: Math.min(
      Math.max(8, next.x),
      Math.max(8, innerWidth - bounds.width - 8)
    ),
    y: Math.min(
      Math.max(62, next.y),
      Math.max(62, innerHeight - bounds.height - 12)
    ),
  }
}
