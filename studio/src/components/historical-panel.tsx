import { useEffect, useMemo, useRef, useState } from "react"
import { History, RefreshCw, Search, X } from "lucide-react"
import { NativeSelect } from "@rostfrei/ui/components/native-select"
import { Input } from "@rostfrei/ui/components/input"
import { Button } from "@/components/ui/button"
import { MessageGraph } from "@/components/message-graph"
import { HistoricalReconstruction } from "@/components/historical-reconstruction"
import { HistoricalTimeline } from "@/components/historical-timeline"
import { advertisedHref } from "@/lib/api"
import { getHistoricalPage, productionHistory } from "@/lib/historical-api"
import { historicalGraph } from "@/lib/historical-graph"
import { timelinePage } from "@/lib/historical-timeline"
import type {
  HistoricalCapability,
  HistoricalPage,
  StudioLayout,
  TracerCatalog,
} from "@/lib/types"

interface Props {
  active: boolean
  open: boolean
  catalog?: TracerCatalog
  layout: StudioLayout
  onClose: () => void
}

type Browse = "aggregates" | "flows"
type Read = {
  href: string
  capability: HistoricalCapability
  browse: Browse
  title: string
  previous: string[]
  aggregateType?: string
  aggregateId?: string
  rootHref?: string
  prior?: HistoricalPage
}

export function HistoricalPanel({
  active,
  open,
  catalog,
  layout,
  onClose,
}: Props) {
  const [production, setProduction] = useState<HistoricalCapability[]>([])
  const [discoveryError, setDiscoveryError] = useState<string>()
  const [scope, setScope] = useState("test")
  const [browse, setBrowse] = useState<Browse>("aggregates")
  const [aggregateType, setAggregateType] = useState("")
  const [aggregateId, setAggregateId] = useState("")
  const [correlationId, setCorrelationId] = useState("")
  const [read, setRead] = useState<Read>()
  const [page, setPage] = useState<HistoricalPage>()
  const [position, setPosition] = useState(0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const [revision, setRevision] = useState(0)
  const request = useRef<AbortController | undefined>(undefined)
  const capabilities = [
    ...(catalog?.historical ?? []).filter((entry) => entry.scope === "test"),
    ...production,
  ]
  const capability =
    capabilities.find((entry) => entry.scope === scope) ?? capabilities[0]
  const selectedType = capability?.aggregateTypes.includes(aggregateType)
    ? aggregateType
    : (capability?.aggregateTypes[0] ?? "")

  useEffect(() => {
    if (!active) return
    const controller = new AbortController()
    void productionHistory(controller.signal)
      .then((entries) => {
        if (!controller.signal.aborted) {
          setProduction(entries)
          setDiscoveryError(undefined)
        }
      })
      .catch((failure: unknown) => {
        if (!controller.signal.aborted) setDiscoveryError(message(failure))
      })
    return () => controller.abort()
  }, [active, catalog])

  useEffect(() => {
    if (!active || !read) return
    const controller = new AbortController()
    request.current = controller
    void getHistoricalPage(read.capability, read.href, controller.signal)
      .then((result) => {
        if (!controller.signal.aborted) {
          const next = read.capability.timelineHref
            ? timelinePage(result, read.prior)
            : result
          setPage(next)
          if (!read.prior) setPosition(next.items.length)
          setError(undefined)
          setBusy(false)
        }
      })
      .catch((failure: unknown) => {
        if (!controller.signal.aborted) {
          setPage(undefined)
          setError(message(failure))
          setBusy(false)
        }
      })
    return () => controller.abort()
  }, [active, read, revision])

  function clear() {
    request.current?.abort()
    setRead(undefined)
    setPage(undefined)
    setError(undefined)
    setBusy(false)
  }
  function load(next: Read) {
    request.current?.abort()
    if (!next.prior) setPage(undefined)
    setError(undefined)
    setBusy(true)
    setRead(next)
  }
  function search(flow?: string, instance = aggregateId) {
    if (!capability) return
    const mode = flow === undefined ? browse : "flows"
    const params = new URLSearchParams()
    if (mode === "aggregates") {
      if (selectedType) params.set("aggregateType", selectedType)
      if (instance) params.set("aggregateId", instance)
    } else if (flow ?? correlationId)
      params.set("correlationId", flow ?? correlationId)
    if (flow !== undefined) {
      setBrowse("flows")
      setCorrelationId(flow)
    }
    const url = new URL(
      advertisedHref(capability.timelineHref ?? capability.messagesHref),
      location.origin
    )
    for (const [name, value] of params) url.searchParams.set(name, value)
    if (capability.timelineHref) url.searchParams.set("limit", "100")
    load({
      href: url.pathname + url.search,
      capability,
      browse: mode,
      title:
        mode === "aggregates"
          ? instance || selectedType
          : flow || correlationId || "Historical message flows",
      previous: [],
      aggregateType: mode === "aggregates" ? selectedType : undefined,
      aggregateId: mode === "aggregates" ? instance : undefined,
    })
  }
  function selectFlow(event: React.MouseEvent<HTMLButtonElement>) {
    search(event.currentTarget.dataset.historicalFlow)
  }
  function selectInstance(event: React.MouseEvent<HTMLButtonElement>) {
    const id = event.currentTarget.dataset.historicalInstance ?? ""
    setAggregateId(id)
    search(undefined, id)
  }
  const timeline = Boolean(read?.capability.timelineHref)
  const allNodes = useMemo(
    () =>
      historicalGraph(
        page?.items ?? [],
        read?.browse === "aggregates",
        timeline
      ),
    [page, read?.browse, timeline]
  )
  const visibleItems = useMemo(
    () =>
      timeline ? (page?.items ?? []).slice(0, position) : (page?.items ?? []),
    [page, timeline, position]
  )
  const nodes = useMemo(
    () =>
      timeline
        ? historicalGraph(visibleItems, read?.browse === "aggregates", true)
        : allNodes,
    [allNodes, visibleItems, read?.browse, timeline]
  )
  const targetVersion = timeline
    ? (visibleItems.filter((item) => item.kind === "domain-event").at(-1)
        ?.streamVersion ?? "0")
    : undefined
  if (!active) return null
  const flows = new Map<string, { name: string; count: number }>()
  for (const item of page?.items ?? []) {
    if (!item.correlationId) continue
    const previous = flows.get(item.correlationId)
    flows.set(item.correlationId, {
      name: previous?.name ?? item.name,
      count: (previous?.count ?? 0) + 1,
    })
  }
  return (
    <>
      <aside
        id="studio-historical-panel"
        className={`studio-sidebar studio-historical-panel ${open ? "studio-sidebar-open" : ""}`}
        inert={!open}
        aria-label="Historical data"
      >
        <div className="sidebar-section-heading">
          <h2>
            <History size={15} /> Historical
          </h2>
          <Button
            className="sidebar-close"
            variant="ghost"
            size="icon"
            onClick={onClose}
            aria-label="Close historical panel"
          >
            <X />
          </Button>
        </div>
        <form
          className="observation-controls historical-controls"
          onSubmit={(event) => {
            event.preventDefault()
            search()
          }}
        >
          <p>Explore retained events and related message flows.</p>
          <label>
            Traffic scope
            <NativeSelect
              aria-label="Historical scope"
              value={capability?.scope ?? ""}
              disabled={!capability}
              onChange={(event) => {
                clear()
                setScope(event.target.value)
                setAggregateType("")
                setAggregateId("")
                setCorrelationId("")
              }}
            >
              {!capability && (
                <option value="">No historical capability</option>
              )}
              {capabilities.map((entry) => (
                <option key={entry.scope} value={entry.scope}>
                  {entry.application} ·{" "}
                  {entry.scope === "test" ? "Test" : "Production"}
                </option>
              ))}
            </NativeSelect>
          </label>
          <div
            className="historical-browse-switch"
            role="group"
            aria-label="Historical browsing"
          >
            {(["aggregates", "flows"] as const).map((mode) => (
              <Button
                key={mode}
                type="button"
                variant="outline"
                size="sm"
                aria-pressed={browse === mode}
                onClick={() => {
                  clear()
                  setBrowse(mode)
                }}
              >
                {mode === "aggregates" ? "Aggregates" : "Message flows"}
              </Button>
            ))}
          </div>
          {browse === "aggregates" ? (
            <>
              <label>
                Aggregate type
                <NativeSelect
                  aria-label="Historical aggregate type"
                  value={selectedType}
                  disabled={!capability}
                  onChange={(event) => {
                    setAggregateType(event.target.value)
                    setAggregateId("")
                  }}
                >
                  {!selectedType && (
                    <option value="">No aggregate types</option>
                  )}
                  {capability?.aggregateTypes.map((type) => (
                    <option key={type} value={type}>
                      {type}
                    </option>
                  ))}
                </NativeSelect>
              </label>
              <label>
                Instance ID
                <Input
                  aria-label="Historical aggregate ID"
                  placeholder="All instances"
                  value={aggregateId}
                  onChange={(event) => setAggregateId(event.target.value)}
                />
              </label>
            </>
          ) : (
            <label>
              Correlation ID
              <Input
                aria-label="Historical correlation ID"
                placeholder="Browse all retained flows"
                value={correlationId}
                onChange={(event) => setCorrelationId(event.target.value)}
              />
            </label>
          )}
          <Button
            type="submit"
            variant="outline"
            disabled={
              !capability || busy || (browse === "aggregates" && !selectedType)
            }
            aria-label="Search historical data"
          >
            <Search /> Inspect history
          </Button>
        </form>
        <div className="observation-flows">
          {discoveryError && (
            <p role="alert" className="observation-notice">
              Production discovery: {discoveryError}
            </p>
          )}
          {!capability && (
            <p className="observation-empty">
              Historical inspection is unavailable. Connect a Tracer host with a
              historical reader. Production requires an inspection token.
            </p>
          )}
          {read?.browse === "flows" &&
            [...flows].map(([id, flow]) => (
              <button
                key={id}
                className="observation-flow"
                data-historical-flow={id}
                onClick={selectFlow}
              >
                <strong>{flow.name}</strong>
                <span className="font-mono">{id}</span>
                <span>
                  {flow.count}{" "}
                  {timeline ? "milestones loaded" : "records on this page"} ·
                  inspect flow
                </span>
              </button>
            ))}
          {read?.browse === "aggregates" &&
            [...new Set(page?.items.map((item) => item.aggregateId))]
              .filter(Boolean)
              .map((id) => (
                <button
                  key={id}
                  className="observation-flow"
                  data-historical-instance={id}
                  onClick={selectInstance}
                >
                  <strong>{id}</strong>
                  <span>
                    {
                      page?.items.filter((item) => item.aggregateId === id)
                        .length
                    }{" "}
                    events {timeline ? "loaded" : "on this page"} · inspect
                    instance
                  </span>
                </button>
              ))}
        </div>
        <footer className="observation-footer">
          <span>Persisted NATS data · read-only</span>
          <span>
            Requests, responses and integration events follow broker retention.
            Reset removes Test history.
          </span>
        </footer>
      </aside>
      <main className="studio-main historical-main">
        <div className="studio-heading">
          <section
            className="observation-heading historical-heading"
            aria-live="polite"
          >
            <div>
              <h1>{read?.title || "Inspect historical data"}</h1>
              <p>
                {read
                  ? `${read.capability.application} · ${read.capability.scope}`
                  : "Choose a scope and inspect aggregate events or message flows."}
              </p>
              <p>Persisted evidence · partial flow coverage</p>
            </div>
            <div className="historical-paging">
              <Button
                variant="ghost"
                size="sm"
                disabled={!read || busy}
                aria-label="Refresh historical data"
                onClick={() => {
                  if (read) {
                    load({
                      ...read,
                      href: read.rootHref ?? read.previous[0] ?? read.href,
                      previous: [],
                      prior: undefined,
                    })
                    setRevision((value) => value + 1)
                  }
                }}
              >
                <RefreshCw /> Refresh
              </Button>
              {!timeline && (
                <>
                  <Button
                    variant="outline"
                    size="sm"
                    aria-label="Previous historical page"
                    disabled={!read?.previous.length || busy}
                    onClick={() => {
                      if (read?.previous.length)
                        load({
                          ...read,
                          href: read.previous.at(-1)!,
                          previous: read.previous.slice(0, -1),
                        })
                    }}
                  >
                    Previous
                  </Button>
                  <span>Page {(read?.previous.length ?? 0) + 1}</span>
                  <Button
                    variant="outline"
                    size="sm"
                    aria-label="Next historical page"
                    disabled={!page?.nextHref || busy}
                    onClick={() => {
                      if (read && page?.nextHref)
                        load({
                          ...read,
                          href: page.nextHref,
                          previous: [...read.previous, read.href],
                        })
                    }}
                  >
                    Next
                  </Button>
                </>
              )}
            </div>
          </section>
          {busy && (
            <p className="historical-notice" role="status">
              Reading retained history…
            </p>
          )}
          {error && (
            <p className="historical-notice" role="alert">
              {error} Use Refresh to start a new read.
            </p>
          )}
          {page && (
            <p className="historical-notice">
              {page.items.length} records · {page.scannedMessages} scanned ·{" "}
              {page.skippedMessages} unreadable
              {page.nextHref
                ? " · more data available"
                : " · end of retained data at this read’s boundaries"}
              . Missing messages may have expired.
            </p>
          )}
          {page && timeline && (
            <HistoricalTimeline
              key={`${read?.rootHref ?? read?.href}:${revision}`}
              items={page.items}
              position={position}
              onSeek={setPosition}
              more={Boolean(page.nextHref)}
              busy={busy}
              onLoadMore={() => {
                if (read && page.nextHref)
                  load({
                    ...read,
                    rootHref: read.rootHref ?? read.href,
                    href: page.nextHref,
                    prior: page,
                  })
              }}
            />
          )}
          {page &&
            read?.browse === "aggregates" &&
            read.aggregateType &&
            read.aggregateId &&
            read.capability.aggregateHref &&
            read.capability.snapshotAggregateTypes?.includes(
              read.aggregateType
            ) && (
              <HistoricalReconstruction
                key={`${read.capability.scope}:${read.aggregateType}:${read.aggregateId}:${page.generation}:${revision}`}
                capability={read.capability}
                aggregateType={read.aggregateType}
                aggregateId={read.aggregateId}
                generation={page.generation}
                timelineVersion={targetVersion}
              />
            )}
        </div>
        <MessageGraph
          key={`${read?.rootHref ?? read?.href ?? "historical-empty"}:${revision}`}
          layoutMode={layout}
          nodes={nodes}
          layoutNodes={timeline ? allNodes : undefined}
          view="historical"
          onInspectCorrelation={(id) => search(id)}
          emptyMessage={
            page
              ? timeline && page.items.length && position === 0
                ? "Before the first recorded milestone. Drag the timeline or press Play."
                : page.nextHref
                  ? "No matching records in this scan. Load more history to continue."
                  : "No retained records match this query."
              : "Inspect history to load persisted events and message flows."
          }
        />
      </main>
    </>
  )
}

function message(failure: unknown): string {
  return failure instanceof Error
    ? failure.message
    : "Historical inspection failed"
}
