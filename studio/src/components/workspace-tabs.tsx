import { useEffect } from "react"
import { workspaceViews, type WorkspaceView } from "@/lib/workspace"

const inspectionViews = workspaceViews.filter(
  (view) => !("requiresBehavioralTests" in view)
)

export function WorkspaceTabs({
  behavioralTests,
  value,
  onChange,
}: {
  behavioralTests: boolean
  value: WorkspaceView
  onChange: (view: WorkspaceView) => void
}) {
  const views = behavioralTests ? workspaceViews : inspectionViews
  useEffect(() => {
    const navigate = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.isComposing ||
        event.repeat ||
        !event.ctrlKey ||
        event.altKey ||
        event.metaKey ||
        event.shiftKey
      )
        return
      if (!/^Digit[1-9]$/.test(event.code)) return
      const index = Number(event.code.slice(-1)) - 1
      const mode = workspaceViews[index]
      if (!mode || !views.some((view) => view.id === mode.id)) return
      event.preventDefault()
      onChange(mode.id)
      document.getElementById(`studio-tab-${mode.id}`)?.focus()
    }
    window.addEventListener("keydown", navigate)
    return () => window.removeEventListener("keydown", navigate)
  }, [onChange, views])

  return (
    <nav className="studio-navigation-bar" aria-label="Studio navigation">
      <div className="workspace-tabs" role="tablist" aria-label="Studio views">
        {views.map((mode, index) => (
          <button
            key={mode.id}
            id={`studio-tab-${mode.id}`}
            type="button"
            role="tab"
            data-workspace-tab={mode.id}
            aria-label={mode.label}
            aria-selected={value === mode.id}
            aria-controls={value === mode.id ? "studio-workspace" : undefined}
            aria-keyshortcuts={`Control+${workspaceViews.indexOf(mode) + 1}`}
            tabIndex={value === mode.id ? 0 : -1}
            title={`${mode.label} (Ctrl+${workspaceViews.indexOf(mode) + 1})`}
            onClick={() => onChange(mode.id)}
            onKeyDown={(event) => {
              const next =
                event.key === "ArrowRight"
                  ? (index + 1) % views.length
                  : event.key === "ArrowLeft"
                    ? (index + views.length - 1) % views.length
                    : event.key === "Home"
                      ? 0
                      : event.key === "End"
                        ? views.length - 1
                        : undefined
              if (next === undefined) return
              event.preventDefault()
              onChange(views[next].id)
              document.getElementById(`studio-tab-${views[next].id}`)?.focus()
            }}
          >
            <mode.icon size={16} aria-hidden="true" />
            <span>{mode.label}</span>
          </button>
        ))}
      </div>
    </nav>
  )
}
