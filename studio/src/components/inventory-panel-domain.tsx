import { useCallback, useMemo, useRef, useState, type ReactNode } from "react"
import { ArrowLeft, Layers } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useMediaQuery } from "@/lib/use-media-query"
import { DomainCanvas } from "./domain-canvas"
import { DomainDetails } from "./domain-details"
import { layoutDomain, scopeDomain, type DomainCard } from "./domain-layout"
import type { DomainInventory } from "./inventory-panel-data"
import "./domain-canvas.css"
import "./domain-details.css"

export function Domain({
  data,
  refresh,
}: {
  data: DomainInventory
  refresh: ReactNode
}) {
  const [scopeId, setScopeId] = useState<string>()
  const compact = useMediaQuery("(max-width: 720px)")
  const [selectedId, setSelectedId] = useState<string>()
  const workspace = useRef<HTMLDivElement>(null)
  const opener = useRef<HTMLButtonElement | null>(null)
  const model = useMemo(() => layoutDomain(data), [data])
  const scope = scopeId ? model.nodes.get(scopeId) : undefined
  const selected = selectedId ? model.nodes.get(selectedId) : undefined
  const layout = useMemo(
    () => scopeDomain(model, scope, compact),
    [model, scope, compact]
  )

  const closeDetails = useCallback(() => {
    setSelectedId(undefined)
    if (opener.current?.isConnected)
      opener.current.focus({ preventScroll: true })
    else workspace.current?.focus({ preventScroll: true })
    opener.current = null
  }, [])

  const changeScope = useCallback((id?: string) => {
    setScopeId(id)
    setSelectedId(undefined)
    opener.current = null
    // A new scope replaces its canvas nodes, including the triggering button.
    workspace.current?.focus({ preventScroll: true })
  }, [])

  const select = useCallback(
    ({ id, navigates }: DomainCard) => {
      if (navigates) {
        changeScope(id)
      } else {
        opener.current =
          workspace.current?.querySelector<HTMLButtonElement>(
            `[data-domain-node=${CSS.escape(id)}]`
          ) ?? null
        setSelectedId(id)
      }
    },
    [changeScope]
  )

  return (
    <div
      className="domain-inventory"
      ref={workspace}
      tabIndex={-1}
      role="group"
      aria-label="Domain explorer"
    >
      <DomainCanvas
        key={scope?.id ?? "overview"}
        layout={layout}
        title={scope?.label ?? "Domain model"}
        selected={selected?.id}
        select={select}
        onPaneClick={selected ? closeDetails : undefined}
        leading={
          scope ? (
            <Button
              size="icon"
              variant="ghost"
              aria-label="Back to Domain model"
              onClick={() => changeScope()}
            >
              <ArrowLeft aria-hidden="true" />
            </Button>
          ) : (
            <Layers size={17} aria-hidden="true" />
          )
        }
        tools={refresh}
      >
        {!data.compiledModelAvailable && !scope && (
          <p className="domain-canvas-notice">
            Compiled model unavailable. Showing runtime registrations.
          </p>
        )}
        {data.compiledModelAvailable && layout.cards.length === 0 && (
          <p className="domain-canvas-notice">No domain nodes were reported.</p>
        )}
        {selected && (
          <DomainDetails
            key={selected.id}
            node={selected}
            model={model}
            data={data}
            onClose={closeDetails}
          />
        )}
      </DomainCanvas>
    </div>
  )
}
