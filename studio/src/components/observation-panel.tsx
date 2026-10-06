import { useEffect, useEffectEvent, useState } from "react"
import { Pause, Play, Radio, X } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  followObservation,
  getObservedFlow,
  productionObservation,
} from "@/lib/observation-api"
import type {
  ObservationCapability,
  ObservationSnapshot,
  ObservedFlow,
  TracerCatalog,
} from "@/lib/types"

export type ObservationConnection =
  ObservationSnapshot["status"] | "paused" | "disconnected"

interface Props {
  open: boolean
  active: boolean
  catalog?: TracerCatalog
  onClose: () => void
  onSelect: () => void
  onFlow: (flow: ObservedFlow | undefined) => void
  onStatus: (status: ObservationConnection) => void
}

export function ObservationPanel({
  open,
  active,
  catalog,
  onClose,
  onSelect,
  onFlow,
  onStatus,
}: Props) {
  const [production, setProduction] = useState<ObservationCapability[]>([])
  const [discoveryError, setDiscoveryError] = useState<string>()
  const [scope, setScope] = useState<ObservationCapability["scope"]>("test")
  const [snapshot, setSnapshot] = useState<ObservationSnapshot>()
  const [connection, setConnection] =
    useState<ObservationConnection>("connecting")
  const [streamError, setStreamError] = useState<string>()
  const [detailError, setDetailError] = useState<string>()
  const [selectedId, setSelectedId] = useState<string>()
  const [filter, setFilter] = useState("")
  const [paused, setPaused] = useState(false)
  const capabilities = [
    ...(catalog?.observation ?? []).filter((entry) => entry.scope === "test"),
    ...production,
  ]
  const capability =
    capabilities.find((entry) => entry.scope === scope) ?? capabilities[0]
  const { application, scope: effectiveScope, eventsHref } = capability ?? {}
  const selected = snapshot?.items.find((item) => item.id === selectedId)
  const detailHref = selected?.detailHref
  const detailRevision = selected?.revision
  const generation = snapshot?.generation
  const status = paused ? "paused" : connection
  const latestDetailRevision = useEffectEvent(() => detailRevision)

  useEffect(() => {
    if (!active) return
    const controller = new AbortController()
    void productionObservation(controller.signal)
      .then((entries) => {
        if (!controller.signal.aborted) {
          setProduction(entries)
          setDiscoveryError(undefined)
        }
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) setDiscoveryError(message(error))
      })
    return () => controller.abort()
  }, [active])

  useEffect(() => {
    if (!active) return
    if (paused || !application || !effectiveScope || !eventsHref) {
      onStatus(paused ? "paused" : "unavailable")
      return
    }
    const controller = new AbortController()
    let retry: number | undefined
    let attempts = 0
    onStatus("connecting")
    const connect = async () => {
      try {
        await followObservation(
          { application, scope: effectiveScope, eventsHref, listHref: "" },
          controller.signal,
          (next) => {
            attempts = 0
            setSnapshot(next)
            setConnection(next.status)
            setStreamError(undefined)
            onStatus(next.status)
          }
        )
      } catch (error) {
        if (controller.signal.aborted) return
        setConnection("disconnected")
        setStreamError(message(error))
        onStatus("disconnected")
        retry = window.setTimeout(
          () => void connect(),
          Math.min(10_000, 1000 * 2 ** Math.min(attempts++, 4))
        )
      }
    }
    void connect()
    return () => {
      controller.abort()
      window.clearTimeout(retry)
    }
  }, [active, paused, application, effectiveScope, eventsHref, onStatus])

  useEffect(() => {
    if (!active || paused || !effectiveScope || !detailHref) return
    const controller = new AbortController()
    let timer: number | undefined
    let fetchedRevision: string | undefined
    const refresh = async () => {
      const revision = latestDetailRevision()
      if (revision !== fetchedRevision) {
        try {
          const flow = await getObservedFlow(
            effectiveScope,
            detailHref,
            controller.signal
          )
          if (controller.signal.aborted) return
          if (flow.scope !== effectiveScope || flow.generation !== generation)
            throw new Error("Observed flow changed; select it again")
          fetchedRevision = revision
          setDetailError(undefined)
          onFlow(flow)
        } catch (error) {
          if (controller.signal.aborted) return
          setDetailError(message(error))
          onFlow(undefined)
        }
      }
      timer = window.setTimeout(() => void refresh(), 500)
    }
    void refresh()
    return () => {
      controller.abort()
      window.clearTimeout(timer)
    }
  }, [active, paused, effectiveScope, detailHref, generation, onFlow])

  const expired = Boolean(selectedId && snapshot && !selected)
  const items = (snapshot?.items ?? []).filter((item) =>
    `${item.name} ${item.correlationId}`
      .toLowerCase()
      .includes(filter.toLowerCase())
  )
  return (
    <aside
      id="studio-observation-panel"
      className={`studio-sidebar studio-observation-panel ${open ? "studio-sidebar-open" : ""}`}
      inert={!open}
      aria-label="Observed application flows"
    >
      <div className="sidebar-section-heading">
        <h2>
          <Radio size={15} /> Observe
        </h2>
        <Button
          variant="ghost"
          size="icon"
          className="sidebar-close"
          onClick={onClose}
          aria-label="Close observation panel"
        >
          <X />
        </Button>
      </div>
      <div className="observation-controls">
        <p>Follow events from your running application.</p>
        <label>
          Traffic scope
          <select
            aria-label="Observation scope"
            value={effectiveScope ?? ""}
            disabled={!capability}
            onChange={(event) => {
              setScope(event.target.value as ObservationCapability["scope"])
              setSnapshot(undefined)
              setSelectedId(undefined)
              setDetailError(undefined)
              setStreamError(undefined)
              setConnection("connecting")
              onFlow(undefined)
              onStatus("connecting")
            }}
          >
            {!capability && <option value="">No observation capability</option>}
            {capabilities.map((entry) => (
              <option key={entry.scope} value={entry.scope}>
                {entry.application} ·{" "}
                {entry.scope === "test" ? "Test" : "Production"}
              </option>
            ))}
          </select>
        </label>
        <div className="flex items-center justify-between gap-2">
          <Badge
            variant={status === "live" ? "live" : "neutral"}
            data-observation-status={capability ? status : "unavailable"}
          >
            {capability ? status : "unavailable"}
          </Badge>
          <Button
            variant="ghost"
            size="sm"
            disabled={!capability}
            aria-label={paused ? "Resume observation" : "Pause observation"}
            onClick={() => {
              setPaused(!paused)
              if (paused) {
                setConnection("connecting")
                onStatus("connecting")
              } else onStatus("paused")
            }}
          >
            {paused ? <Play /> : <Pause />}
            {paused ? "Resume" : "Pause"}
          </Button>
        </div>
        <input
          aria-label="Filter observed flows"
          placeholder="Filter name or correlation…"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </div>
      <div className="observation-flows">
        {(discoveryError || streamError || detailError) && (
          <p className="observation-notice" role="alert">
            {discoveryError || streamError || detailError}
          </p>
        )}
        {expired && (
          <p className="observation-notice" role="status">
            The selected flow was evicted or reset. Its displayed graph is a
            retained snapshot; select another flow.
          </p>
        )}
        {connection === "unavailable" && capability && (
          <p className="observation-notice">
            An application observer is unavailable. The retained window may have
            gaps.
          </p>
        )}
        {connection === "resetting" && (
          <p className="observation-notice">
            Test state is resetting. Waiting for observation to resume.
          </p>
        )}
        {!capability && (
          <p className="observation-empty">
            Enable continuous observation in your Tracer host. Production
            discovery also requires a read-only inspection token.
          </p>
        )}
        {capability && items.length === 0 && (
          <p className="observation-empty">
            {filter
              ? "No matching flows in the retained window."
              : "Waiting for application events. Trigger an action in the application to see its flow here."}
          </p>
        )}
        {items.map((item) => (
          <button
            type="button"
            key={item.id}
            data-observed-flow={item.id}
            className={`observation-flow ${selectedId === item.id ? "observation-flow-selected" : ""}`}
            aria-pressed={selectedId === item.id}
            onClick={() => {
              setSelectedId(item.id)
              setDetailError(undefined)
              if (selectedId !== item.id) onFlow(undefined)
              if (paused) setPaused(false)
              onSelect()
            }}
          >
            <strong>{item.name}</strong>
            <span className="font-mono">{item.correlationId}</span>
            <span>
              {item.messageCount} messages{item.truncated ? " · truncated" : ""}
              {item.conflicted ? " · conflicting evidence" : ""}
            </span>
          </button>
        ))}
      </div>
      <footer className="observation-footer">
        <span>
          Rolling memory window · {snapshot?.items.length ?? 0}/
          {snapshot?.maximumFlows ?? 128} flows
        </span>
        {snapshot &&
          (snapshot.evictedFlows !== "0" ||
            snapshot.discardedMessages !== "0") && (
            <span className="text-amber-200">
              {snapshot.evictedFlows} evicted · {snapshot.discardedMessages}{" "}
              messages omitted
            </span>
          )}
        <span>Capture is partial; silence does not mean completion.</span>
      </footer>
    </aside>
  )
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Observation failed"
}
