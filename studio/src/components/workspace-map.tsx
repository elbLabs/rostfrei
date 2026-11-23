import { useRef } from "react"
import { Compass, Map as MapIcon, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { workspaceViews, type WorkspaceView } from "@/lib/workspace"
import "./workspace-map.css"

export function WorkspaceMap({
  value,
  behavioralTests,
  onChange,
}: {
  value: WorkspaceView
  behavioralTests: boolean
  onChange: (view: WorkspaceView) => void
}) {
  const dialog = useRef<HTMLDialogElement>(null)
  const visible = workspaceViews.filter(
    (view) => behavioralTests || !("requiresBehavioralTests" in view)
  )
  const destinations = visible.map((view, index) => {
    const angle = (index * 2 * Math.PI) / visible.length - Math.PI / 2
    return { view, x: 50 + 35 * Math.cos(angle), y: 50 + 39 * Math.sin(angle) }
  })
  function visit(id: WorkspaceView) {
    dialog.current?.close()
    onChange(id)
    document.getElementById(`studio-tab-${id}`)?.focus()
  }
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="studio-map-trigger"
        aria-haspopup="dialog"
        aria-label="Open Studio map"
        title="Open Studio map"
        onClick={() => dialog.current?.showModal()}
      >
        <MapIcon aria-hidden="true" />
        <span>Studio map</span>
      </Button>
      <dialog
        ref={dialog}
        data-workspace-map
        aria-labelledby="studio-map-title"
        className="studio-map-dialog"
        onKeyDown={(event) => {
          // The obscured canvas must not handle shortcuts while the map has focus.
          event.stopPropagation()
          if (
            event.ctrlKey &&
            !event.altKey &&
            !event.metaKey &&
            !event.shiftKey &&
            /^Digit[1-9]$/.test(event.code)
          ) {
            event.preventDefault()
            const view = workspaceViews[Number(event.code.slice(-1)) - 1]
            if (view && visible.some((item) => item.id === view.id))
              visit(view.id)
          }
        }}
      >
        <header className="studio-map-tools">
          <MapIcon size={16} aria-hidden="true" />
          <h2 id="studio-map-title">Studio map</h2>
          <Button
            size="icon"
            variant="ghost"
            aria-label="Close Studio map"
            onClick={() => dialog.current?.close()}
          >
            <X />
          </Button>
        </header>
        <nav className="studio-map-orbit" aria-label="Workspace destinations">
          <svg aria-hidden="true" width="100%" height="100%">
            {destinations.map(({ view, x, y }) => (
              <line key={view.id} x1="50%" y1="50%" x2={`${x}%`} y2={`${y}%`} />
            ))}
          </svg>
          <div className="studio-map-center">
            <Compass aria-hidden="true" />
            <strong>Studio</strong>
          </div>
          {destinations.map(({ view, x, y }) => (
            <button
              type="button"
              key={view.id}
              className="studio-map-destination"
              style={{ left: `${x}%`, top: `${y}%` }}
              aria-current={view.id === value ? "page" : undefined}
              data-map-view={view.id}
              onClick={() => visit(view.id)}
            >
              <view.icon aria-hidden="true" size={18} />
              <span>{view.label}</span>
            </button>
          ))}
        </nav>
      </dialog>
    </>
  )
}
