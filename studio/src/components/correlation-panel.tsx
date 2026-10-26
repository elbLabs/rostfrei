import { useCallback, useEffect, useRef, useState, type FormEvent } from "react"
import { LoaderCircle, RefreshCw, Unplug, Waypoints, X } from "lucide-react"
import { Input } from "@rostfrei/ui/components/input"
import { NativeSelect } from "@rostfrei/ui/components/native-select"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DEFAULT_CORRELATION_EVENTS_TEMPLATE,
  anchorHref,
  correlationFailure,
  discoverCorrelations,
  getAnchorCorrelations,
  getCorrelationChain,
  inspectionConfigured,
  type CorrelationFailure,
} from "@/lib/correlation-api"
import type {
  AnchorCorrelationView,
  CorrelationCapability,
  CorrelationChain,
  CorrelationFreshness,
  CorrelationRef,
  IndexedCorrelationChain,
} from "@/lib/types"
import { cn } from "@/lib/utils"

export type CorrelationConnection =
  "connecting" | "live" | "unavailable" | "disconnected"

export interface CorrelationProblem {
  failure: CorrelationFailure
  subject: string
  aggregateType?: string
  message: string
}

export interface CorrelationSelection {
  anchor?: { label: string; aggregateType: string; aggregateId: string }
  correlationId?: string
  chain?: IndexedCorrelationChain
  loading: boolean
  problem?: CorrelationProblem
}

interface Props {
  open: boolean
  active: boolean
  onClose: () => void
  onSelect: () => void
  onSelection: (selection: CorrelationSelection) => void
  onStatus: (status: CorrelationConnection) => void
}

const CUSTOM_ANCHOR = ""

export function CorrelationPanel({
  open,
  active,
  onClose,
  onSelect,
  onSelection,
  onStatus,
}: Props) {
  const [capability, setCapability] = useState<CorrelationCapability>()
  const [discovery, setDiscovery] =
    useState<CorrelationConnection>("connecting")
  const [discoveryError, setDiscoveryError] = useState<string>()
  const [discoveryAttempt, setDiscoveryAttempt] = useState(0)
  const [anchorType, setAnchorType] = useState(CUSTOM_ANCHOR)
  const [customType, setCustomType] = useState("")
  const [aggregateId, setAggregateId] = useState("")
  const [directId, setDirectId] = useState("")
  const [view, setView] = useState<AnchorCorrelationView>()
  const [viewLoading, setViewLoading] = useState(false)
  const [viewProblem, setViewProblem] = useState<CorrelationProblem>()
  const [selectedId, setSelectedId] = useState<string>()
  const [included, setIncluded] = useState<string[]>([])
  const controller = useRef<AbortController | undefined>(undefined)
  const chainController = useRef<AbortController | undefined>(undefined)
  const configured = inspectionConfigured()
  const anchors = capability?.anchors ?? []
  const anchor = anchors.find((entry) => entry.aggregateType === anchorType)
  const aggregateType = anchor?.aggregateType ?? customType.trim()
  const eventsTemplate =
    capability?.eventsHrefTemplate ?? DEFAULT_CORRELATION_EVENTS_TEMPLATE

  useEffect(() => {
    if (!active) return
    if (!configured) {
      onStatus("unavailable")
      return
    }
    const discoveryController = new AbortController()
    onStatus("connecting")
    void discoverCorrelations(discoveryController.signal)
      .then((discovered) => {
        if (discoveryController.signal.aborted) return
        setCapability(discovered)
        setDiscoveryError(undefined)
        setAnchorType(
          (current) =>
            discovered?.anchors.find((entry) => entry.aggregateType === current)
              ?.aggregateType ??
            discovered?.anchors[0]?.aggregateType ??
            CUSTOM_ANCHOR
        )
        const status = discovered ? "live" : "unavailable"
        setDiscovery(status)
        onStatus(status)
      })
      .catch((error: unknown) => {
        if (discoveryController.signal.aborted) return
        setDiscovery("disconnected")
        setDiscoveryError(message(error))
        onStatus("disconnected")
      })
    return () => discoveryController.abort()
  }, [active, configured, discoveryAttempt, onStatus])

  useEffect(
    () => () => {
      controller.current?.abort()
      chainController.current?.abort()
    },
    []
  )

  const loadChain = useCallback(
    async (
      correlationId: string,
      source?: { view: AnchorCorrelationView; label: string }
    ) => {
      chainController.current?.abort()
      const chainRequest = new AbortController()
      chainController.current = chainRequest
      const anchorIdentity = source && {
        label: source.label,
        aggregateType: source.view.aggregateType,
        aggregateId: source.view.aggregateId,
      }
      onSelection({ anchor: anchorIdentity, correlationId, loading: true })
      try {
        const chain = await getCorrelationChain(
          correlationId,
          chainRequest.signal,
          eventsTemplate
        )
        if (chainRequest.signal.aborted) return
        const anchored = source?.view.correlations.find(
          (entry) => entry.correlationId === correlationId
        )
        onSelection({
          anchor: anchorIdentity,
          correlationId,
          chain: withAnchorPayloads(chain, anchored),
          loading: false,
        })
      } catch (error) {
        if (chainRequest.signal.aborted) return
        onSelection({
          anchor: anchorIdentity,
          correlationId,
          loading: false,
          problem: {
            failure: correlationFailure(error),
            subject: correlationId,
            message: message(error),
          },
        })
      }
    },
    [eventsTemplate, onSelection]
  )

  const loadAnchor = async (
    reloadSelection: boolean,
    includedContexts = reloadSelection ? included : []
  ) => {
    const id = aggregateId.trim()
    if (!aggregateType || !id) return
    controller.current?.abort()
    const anchorRequest = new AbortController()
    controller.current = anchorRequest
    setViewLoading(true)
    setViewProblem(undefined)
    setIncluded(includedContexts)
    if (!reloadSelection) {
      setView(undefined)
      setSelectedId(undefined)
      onSelection({ loading: false })
    }
    try {
      const loaded = await getAnchorCorrelations(
        anchorHref(
          aggregateType,
          id,
          anchor?.correlationsHrefTemplate,
          includedContexts
        ),
        anchorRequest.signal
      )
      if (anchorRequest.signal.aborted) return
      setView(loaded)
      if (reloadSelection && selectedId)
        void loadChain(selectedId, {
          view: loaded,
          label: anchor?.label ?? aggregateType,
        })
    } catch (error) {
      if (anchorRequest.signal.aborted) return
      setView(undefined)
      setViewProblem({
        failure: correlationFailure(error),
        subject: `${aggregateType} ${id}`,
        aggregateType,
        message: message(error),
      })
    } finally {
      if (!anchorRequest.signal.aborted) setViewLoading(false)
    }
  }

  const submitAnchor = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    void loadAnchor(false)
  }

  const submitCorrelation = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const id = directId.trim()
    if (!id) return
    setSelectedId(undefined)
    void loadChain(id)
    onSelect()
  }

  const status = configured ? discovery : "unavailable"
  const ready = status === "live"
  const busy = viewLoading
  return (
    <aside
      id="studio-correlation-panel"
      className={cn(
        "studio-sidebar studio-observation-panel studio-correlation-panel",
        open && "studio-sidebar-open"
      )}
      inert={!open}
      aria-label="Correlations by business object"
    >
      <div className="sidebar-section-heading">
        <h2>
          <Waypoints size={15} /> Correlations
        </h2>
        <Button
          variant="ghost"
          size="icon"
          className="sidebar-close"
          onClick={onClose}
          aria-label="Close correlations panel"
        >
          <X />
        </Button>
      </div>
      <form className="observation-controls" onSubmit={submitAnchor}>
        <p>
          Follow every chain that touched one business object, across bounded
          contexts.
        </p>
        {anchors.length > 0 && (
          <label>
            Anchor
            <NativeSelect
              aria-label="Anchor aggregate type"
              value={anchorType}
              disabled={!ready}
              onChange={(event) => setAnchorType(event.target.value)}
            >
              {anchors.map((entry) => (
                <option key={entry.aggregateType} value={entry.aggregateType}>
                  {entry.label} · {entry.aggregateType}
                </option>
              ))}
              <option value={CUSTOM_ANCHOR}>Other aggregate type…</option>
            </NativeSelect>
          </label>
        )}
        {!anchor && (
          <label>
            {anchors.length > 0
              ? "Other aggregate type"
              : "Anchor aggregate type"}
            <Input
              aria-label={
                anchors.length > 0
                  ? "Other anchor aggregate type"
                  : "Anchor aggregate type"
              }
              placeholder="context/aggregate"
              value={customType}
              disabled={!ready}
              onChange={(event) => setCustomType(event.target.value)}
            />
          </label>
        )}
        <label>
          Aggregate ID
          <Input
            aria-label="Anchor aggregate ID"
            placeholder="Stream ID of the anchor"
            value={aggregateId}
            disabled={!ready}
            onChange={(event) => setAggregateId(event.target.value)}
          />
        </label>
        <Button
          type="submit"
          size="sm"
          disabled={!ready || busy || !aggregateType || !aggregateId.trim()}
        >
          {busy ? <LoaderCircle className="animate-spin" /> : <Waypoints />}
          Show correlations
        </Button>
      </form>
      <div className="observation-flows correlation-list">
        <CorrelationDiscoveryNotice
          configured={configured}
          discovery={status}
          error={discoveryError}
          anchorsAdvertised={anchors.length > 0}
          onRetry={() => {
            setDiscovery("connecting")
            setDiscoveryAttempt((attempt) => attempt + 1)
          }}
        />
        {viewProblem && (
          <div className="observation-notice correlation-notice" role="alert">
            <strong>{failureTitle(viewProblem.failure)}</strong>
            <span>{failureDetail(viewProblem)}</span>
            <code>{viewProblem.message}</code>
          </div>
        )}
        {view && (
          <div className="correlation-list-heading">
            <span>
              <strong>
                {anchor?.label ?? view.aggregateType} · {view.aggregateId}
              </strong>
              {view.correlations.length}{" "}
              {view.correlations.length === 1 ? "correlation" : "correlations"}
              {view.truncated ? " · older ones omitted" : ""}
            </span>
            <Button
              variant="ghost"
              size="icon"
              disabled={busy}
              aria-label="Refresh anchor correlations"
              onClick={() => void loadAnchor(true)}
            >
              <RefreshCw />
            </Button>
          </div>
        )}
        {view && foreignContexts(view).length > 0 && (
          <fieldset className="correlation-include" disabled={busy}>
            <legend>Load payloads from</legend>
            {foreignContexts(view).map((context) => (
              <label key={context}>
                <input
                  type="checkbox"
                  checked={included.includes(context)}
                  onChange={(event) =>
                    void loadAnchor(
                      true,
                      event.target.checked
                        ? [...included, context].sort()
                        : included.filter((entry) => entry !== context)
                    )
                  }
                />
                {context}
              </label>
            ))}
            <small>Replays each touched stream of that context.</small>
          </fieldset>
        )}
        {view && view.correlations.length === 0 && (
          <p className="observation-empty" role="status">
            No correlations for this anchor. Its events carry none, as with
            fixture or import appends, or the Tracer predates
            unknown-anchor-stream and no stream has this ID.
          </p>
        )}
        {view?.correlations.map((correlation) => {
          const summary = summarize(correlation)
          return (
            <button
              type="button"
              key={correlation.correlationId}
              data-correlation-id={correlation.correlationId}
              className={cn(
                "observation-flow correlation-row",
                selectedId === correlation.correlationId &&
                  "observation-flow-selected"
              )}
              aria-pressed={selectedId === correlation.correlationId}
              onClick={() => {
                setSelectedId(correlation.correlationId)
                void loadChain(correlation.correlationId, {
                  view,
                  label: anchor?.label ?? view.aggregateType,
                })
                onSelect()
              }}
            >
              <strong>{summary.title}</strong>
              <span className="font-mono">{correlation.correlationId}</span>
              <span>
                {summary.events}
                {correlation.truncated ? " · truncated" : ""}
              </span>
              {summary.contexts && <span>{summary.contexts}</span>}
              {summary.period && <span>{summary.period}</span>}
            </button>
          )
        })}
        <form
          className="correlation-direct"
          onSubmit={submitCorrelation}
          aria-label="Open a correlation by ID"
        >
          <label>
            Correlation ID
            <span>
              <Input
                aria-label="Correlation ID"
                placeholder="Open any indexed correlation"
                value={directId}
                disabled={!ready}
                onChange={(event) => setDirectId(event.target.value)}
              />
              <Button
                type="submit"
                variant="outline"
                size="sm"
                disabled={!ready || !directId.trim()}
              >
                Open
              </Button>
            </span>
          </label>
        </form>
      </div>
      <footer className="observation-footer correlation-footer">
        {view ? (
          <FreshnessList freshness={view.freshness} />
        ) : (
          <span>Index freshness appears with the first result.</span>
        )}
        <span>
          0 behind means indexed as of the request, not that a chain has
          finished.
        </span>
      </footer>
    </aside>
  )
}

export function CorrelationHeading({
  selection,
  status,
}: {
  selection: CorrelationSelection
  status: CorrelationConnection
}) {
  const { anchor, chain, correlationId, loading, problem } = selection
  if (problem)
    return (
      <section
        className="connection-notice correlation-chain-notice"
        data-connection="correlation-unavailable"
        data-correlation-failure={problem.failure}
        role="alert"
      >
        <div className="connection-notice-heading">
          <Unplug size={20} />
          <div>
            <h1>{failureTitle(problem.failure)}</h1>
            <p>{failureDetail(problem)}</p>
          </div>
        </div>
        <p className="connection-error-detail">{problem.message}</p>
      </section>
    )
  const summary = chain && summarize(chain)
  const pending = chain && chain.refs.length === 0
  return (
    <section
      className="observation-heading correlation-heading"
      aria-live="polite"
      aria-busy={loading}
    >
      <div>
        <h1>
          {correlationId ??
            (status === "unavailable" || status === "disconnected"
              ? "Correlations unavailable"
              : "Follow a business object")}
        </h1>
        <p>
          {loading
            ? "Loading the indexed chain…"
            : chain
              ? [
                  anchor && `Anchor ${anchor.label} · ${anchor.aggregateId}`,
                  pending ? "No events indexed yet" : summary?.events,
                  summary?.contexts,
                  chain.truncated && "truncated at 2048 references",
                ]
                  .filter(Boolean)
                  .join(" · ")
              : "Choose an anchor and its aggregate ID, then select a correlation."}
        </p>
      </div>
      {chain && (
        <div className="observation-evidence correlation-freshness">
          <FreshnessList freshness={chain.freshness} />
          <span>
            {pending
              ? "The index has no events for this correlation yet; refresh after sources catch up."
              : "Freshness as of this request; later events can still join the chain."}
          </span>
        </div>
      )}
    </section>
  )
}

function CorrelationDiscoveryNotice({
  configured,
  discovery,
  error,
  anchorsAdvertised,
  onRetry,
}: {
  configured: boolean
  discovery: CorrelationConnection
  error?: string
  anchorsAdvertised: boolean
  onRetry: () => void
}) {
  if (!configured)
    return (
      <p className="observation-empty" role="status">
        Correlation views read production history and need Tracer’s read-only
        inspection capability. Set <code>VITE_TRACER_INSPECTION_TOKEN</code> to
        the Tracer inspection token and restart Studio.
      </p>
    )
  if (discovery === "connecting")
    return (
      <p className="observation-empty" role="status">
        Discovering anchors…
      </p>
    )
  if (discovery === "disconnected")
    return (
      <div className="observation-notice correlation-notice" role="alert">
        <strong>Correlation discovery failed</strong>
        <code>{error}</code>
        <Button variant="outline" size="sm" onClick={onRetry}>
          <RefreshCw /> Retry discovery
        </Button>
      </div>
    )
  if (discovery === "unavailable")
    return (
      <p className="observation-empty" role="status">
        This Tracer does not advertise correlation views. Enable the correlation
        index on its Normal runtime.
      </p>
    )
  return anchorsAdvertised ? null : (
    <p className="observation-empty" role="status">
      No anchor aggregates are declared. Enter an aggregate type, or declare
      <code> ANCHOR = true</code> on an aggregate definition.
    </p>
  )
}

function FreshnessList({ freshness }: { freshness: CorrelationFreshness }) {
  if (!freshness.contexts.length)
    return (
      <span data-freshness="unknown">
        Index freshness unknown: no projector has reported a source yet.
      </span>
    )
  const current = freshness.contexts
    .filter((entry) => entry.behind === 0)
    .map((entry) => entry.context)
  const grouped = freshness.contexts.length > 3 && current.length > 1
  return (
    <ul className="freshness-list" aria-label="Correlation index freshness">
      {grouped && (
        <li
          data-freshness="current"
          data-freshness-group={current.join(" ")}
          title={current.join(", ")}
        >
          <span>
            {current.length === freshness.contexts.length
              ? `All ${current.length} contexts`
              : `${current.length} other contexts`}
          </span>
          <Badge variant="success">lag 0</Badge>
        </li>
      )}
      {freshness.contexts.map((entry) => {
        const state =
          entry.behind === null
            ? "unknown"
            : entry.behind === 0
              ? "current"
              : "behind"
        if (grouped && state === "current") return null
        return (
          <li
            key={entry.context}
            data-freshness-context={entry.context}
            data-freshness={state}
          >
            <span>{entry.context}</span>
            <Badge
              variant={
                state === "current"
                  ? "success"
                  : state === "behind"
                    ? "warning"
                    : "neutral"
              }
            >
              {state === "current"
                ? "lag 0"
                : state === "behind"
                  ? `${entry.behind} behind`
                  : "unknown"}
            </Badge>
            {state === "unknown" && (
              <small>
                {entry.sources.some((source) => source.state === "inactive")
                  ? "Projector inactive"
                  : "Lag unknown; projector may be inactive"}
              </small>
            )}
          </li>
        )
      })}
    </ul>
  )
}

function foreignContexts(view: AnchorCorrelationView): string[] {
  const anchorContext = view.aggregateType.split("/")[0]
  return [
    ...new Set(
      view.correlations.flatMap((correlation) =>
        correlation.refs
          .filter((reference) => reference.kind === "domain")
          .map((reference) => reference.context)
      )
    ),
  ]
    .filter((context) => context !== anchorContext)
    .sort()
}

function withAnchorPayloads(
  chain: IndexedCorrelationChain,
  anchored?: CorrelationChain
): IndexedCorrelationChain {
  const payloads = new Map<string, unknown>()
  for (const reference of anchored?.refs ?? [])
    if (reference.kind === "domain" && reference.payload !== undefined)
      payloads.set(
        `${reference.context}\u0000${reference.eventId}`,
        reference.payload
      )
  if (!payloads.size) return chain
  return {
    ...chain,
    refs: chain.refs.map((reference) =>
      reference.kind === "domain" && reference.payload === undefined
        ? {
            ...reference,
            payload: payloads.get(
              `${reference.context}\u0000${reference.eventId}`
            ),
          }
        : reference
    ),
  }
}

function summarize(correlation: CorrelationChain) {
  const refs = correlation.refs
  const domain = refs.filter((reference) => reference.kind === "domain").length
  const integration = refs.length - domain
  const contexts = [...new Set(refs.map((reference) => reference.context))]
  const first = refs[0]
  const last = refs.at(-1)
  return {
    title:
      (
        refs.find(
          (reference) =>
            reference.causationId ===
            (reference.kind === "domain"
              ? reference.eventId
              : reference.messageId)
        ) ?? first
      )?.eventType ?? "Not indexed yet",
    events: `${refs.length} ${refs.length === 1 ? "event" : "events"} · ${domain} domain · ${integration} integration`,
    contexts: contexts.length
      ? `${contexts.length === 1 ? "Context" : `${contexts.length} contexts`}: ${contexts.join(", ")}`
      : undefined,
    period:
      first && last
        ? first === last
          ? `At ${timestamp(first)}`
          : `${timestamp(first)} → ${timestamp(last)}`
        : undefined,
  }
}

function timestamp(reference: CorrelationRef): string {
  return new Date(reference.occurredAt)
    .toISOString()
    .replace("T", " ")
    .replace(/\.\d+Z$/, "Z")
}

function failureTitle(failure: CorrelationFailure): string {
  switch (failure) {
    case "credential-missing":
      return "Inspection token not configured"
    case "forbidden":
      return "Inspection capability required"
    case "unknown-anchor":
      return "Not an anchor aggregate"
    case "unknown-anchor-stream":
      return "No stream for this anchor ID"
    case "not-indexed":
      return "Correlation not indexed yet"
    case "index-unavailable":
      return "Correlation index unavailable"
    case "invalid-identifier":
      return "Invalid identifier"
    case "request-failed":
      return "Correlation request failed"
  }
}

function failureDetail({
  failure,
  subject,
  aggregateType,
}: CorrelationProblem): string {
  switch (failure) {
    case "credential-missing":
      return "Set VITE_TRACER_INSPECTION_TOKEN to the Tracer inspection token and restart Studio."
    case "forbidden":
      return "Tracer rejected the inspection token. Correlation views require the read-only production inspection capability."
    case "unknown-anchor":
      return `${aggregateType ?? subject} is not a declared anchor type. Declare ANCHOR = true on its aggregate definition.`
    case "unknown-anchor-stream":
      return `No events are stored for ${subject}. Use the aggregate ID of the anchor stream, not the ID of an entity inside it or of a related object.`
    case "not-indexed":
      return `The index has no events for ${subject} and no source is behind. Check the ID; Tracer only indexes Normal traffic, not isolated Test runs.`
    case "index-unavailable":
      return "Tracer could not read the correlation index or event history. Check the projector and production history configuration."
    case "invalid-identifier":
      return "Tracer rejected the identifier. Check its spelling."
    case "request-failed":
      return "The request did not complete. Check the Tracer connection and retry."
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Correlation request failed"
}
