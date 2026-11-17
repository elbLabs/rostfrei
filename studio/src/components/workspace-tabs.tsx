import { useEffect } from "react"
import { workspaceModes, type Workspace } from "@/lib/workspace"

export function WorkspaceTabs({
  value,
  onChange,
}: {
  value: Workspace
  onChange: (workspace: Workspace) => void
}) {
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
      const index = Number(event.code.replace(/^Digit/, "")) - 1
      const mode = workspaceModes[index]
      if (!mode) return
      event.preventDefault()
      onChange(mode.id)
      document.getElementById(`studio-tab-${mode.id}`)?.focus()
    }
    window.addEventListener("keydown", navigate)
    return () => window.removeEventListener("keydown", navigate)
  }, [onChange])

  return (
    <nav className="studio-navigation-bar" aria-label="Studio modes">
      <div className="workspace-tabs" role="tablist" aria-label="Workspace">
        {workspaceModes.map((mode, index) => (
          <button
            key={mode.id}
            id={`studio-tab-${mode.id}`}
            type="button"
            role="tab"
            data-workspace-tab={mode.id}
            aria-label={mode.label}
            aria-selected={value === mode.id}
            aria-controls="studio-workspace"
            aria-keyshortcuts={`Control+${index + 1}`}
            tabIndex={value === mode.id ? 0 : -1}
            title={`${mode.label} (Ctrl+${index + 1})`}
            onClick={() => onChange(mode.id)}
            onKeyDown={(event) => {
              const next =
                event.key === "ArrowRight"
                  ? (index + 1) % workspaceModes.length
                  : event.key === "ArrowLeft"
                    ? (index + workspaceModes.length - 1) %
                      workspaceModes.length
                    : event.key === "Home"
                      ? 0
                      : event.key === "End"
                        ? workspaceModes.length - 1
                        : undefined
              if (next === undefined) return
              event.preventDefault()
              onChange(workspaceModes[next].id)
              document
                .getElementById(`studio-tab-${workspaceModes[next].id}`)
                ?.focus()
            }}
          >
            <mode.icon size={16} />
            <span>{mode.label}</span>
            <kbd aria-hidden="true">{index + 1}</kbd>
          </button>
        ))}
      </div>
    </nav>
  )
}
