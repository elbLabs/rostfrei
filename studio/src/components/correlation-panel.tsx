import { useCallback, useEffect, useRef, useState, type FormEvent } from "react"
import { LoaderCircle, RefreshCw, Unplug, Waypoints, X } from "lucide-react"
import { Input } from "@rostfrei/ui/components/input"
import { NativeSelect } from "@rostfrei/ui/components/native-select"
import { MatchBuilder, MatchedChips } from "@/components/correlation-match"
import {
  matchRows,
  readStoredMatches,
  rowMatches,
  storeMatches,
  type MatchRow,
} from "@/lib/correlation-match"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DEFAULT_CORRELATION_EVENTS_TEMPLATE,
  anchorHref,
  correlationFailure,
  discoverCorrelations,
  getAnchorCorrelations,
  getCorrelationChain,
  formatPayloadMatch,
  inspectionConfigured,
  payloadMatchProblem,
  type CorrelationFailure,
} from "@/lib/correlation-api"
import type {
  AnchorCorrelationView,
  CorrelationCapability,
  CorrelationChain,
  CorrelationSnapshot,
  CorrelationRef,
  RetainedCorrelationChain,
  PayloadMatch,
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
  chain?: RetainedCorrelationChain
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
  const [rows, setRows] = useState<MatchRow[]>([])
  const [includeNote, setIncludeNote] = useState<string>()
  const controller = useRef<AbortController | undefined>(undefined)
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
        setAnchorType((current) => {
          const next =
            discovered?.anchors.find((entry) => entry.aggregateType === current)
              ?.aggregateType ??
            discovered?.anchors[0]?.aggregateType ??
            CUSTOM_ANCHOR
          if (next !== current) setRows(matchRows(readStoredMatches(next)))
          return next
        })
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
    },
    []
  )

  const loadChain = useCallback(
    async (
      correlationId: string,
      source?: { view: AnchorCorrelationView; label: string }
    ) => {
      controller.current?.abort()
      setViewLoading(false)
      const anchorIdentity = source && {
        label: source.label,
        aggregateType: source.view.aggregateType,
        aggregateId: source.view.aggregateId,
      }
      if (source) {
        const chain = source.view.correlations.find(
          (entry) => entry.correlationId === correlationId
        )
        onSelection(
          chain
            ? {
                anchor: anchorIdentity,
                correlationId,
                chain: { ...chain, snapshot: source.view.snapshot },
                loading: false,
              }
            : { loading: false }
        )
        return
      }
      const chainRequest = new AbortController()
      controller.current = chainRequest
      onSelection({ correlationId, loading: true })
      try {
        const chain = await getCorrelationChain(
          correlationId,
          chainRequest.signal,
          eventsTemplate
        )
        if (chainRequest.signal.aborted) return
        onSelection({
          correlationId,
          chain,
          loading: false,
        })
      } catch (error) {
        if (chainRequest.signal.aborted) return
        onSelection({
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

  const anchorContext = aggregateType.split("/")[0]
  const matches = rowMatches(rows)
  const matchesValid = matches.every((match) => !payloadMatchProblem(match))

  const loadAnchor = async (
    reloadSelection: boolean,
    includedContexts = included,
    appliedMatches = matches
  ) => {
    const id = aggregateId.trim()
    if (!aggregateType || !id) return
    controller.current?.abort()
    const anchorRequest = new AbortController()
    controller.current = anchorRequest
    setViewLoading(true)
    setViewProblem(undefined)
    setIncluded(includedContexts)
    onSelection({
      correlationId: reloadSelection ? selectedId : undefined,
      loading: true,
    })
    if (!reloadSelection) {
      setView(undefined)
      setSelectedId(undefined)
    }
    try {
      const loaded = await getAnchorCorrelations(
        anchorHref(
          aggregateType,
          id,
          anchor?.correlationsHrefTemplate,
          includedContexts,
          appliedMatches
        ),
        anchorRequest.signal
      )
      if (anchorRequest.signal.aborted) return
      setView(loaded)
      const selected =
        reloadSelection &&
        loaded.correlations.find((entry) => entry.correlationId === selectedId)
      if (selected) {
        onSelection({
          anchor: {
            label: anchor?.label ?? aggregateType,
            aggregateType: loaded.aggregateType,
            aggregateId: loaded.aggregateId,
          },
          correlationId: selected.correlationId,
          chain: { ...selected, snapshot: loaded.snapshot },
          loading: false,
        })
      } else {
        setSelectedId(undefined)
        onSelection({ loading: false })
      }
    } catch (error) {
      if (anchorRequest.signal.aborted) return
      setView(undefined)
      const problem = {
        failure: correlationFailure(error),
        subject: `${aggregateType} ${id}`,
        aggregateType,
        message: message(error),
      }
      setSelectedId(undefined)
      setViewProblem(problem)
      onSelection({ loading: false, problem })
    } finally {
      if (!anchorRequest.signal.aborted) setViewLoading(false)
    }
  }

  const submitAnchor = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!matchesValid) return
    const missing = [
      ...new Set(
        matches
          .map((match) => match.context)
          .filter(
            (context) =>
              context !== anchorContext && !included.includes(context)
          )
      ),
    ]
    setIncludeNote(
      missing.length
        ? `Included ${missing.join(", ")} so ${missing.length === 1 ? "its" : "their"} payloads can be matched.`
        : undefined
    )
    storeMatches(aggregateType, matches)
    void loadAnchor(false, [...included, ...missing].sort(), matches)
  }

  const applyMatches = (next: PayloadMatch[]) => {
    setRows(matchRows(next))
    setIncludeNote(undefined)
    storeMatches(aggregateType, next)
    void loadAnchor(true, included, next)
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
              onChange={(event) => {
                setAnchorType(event.target.value)
                setRows(matchRows(readStoredMatches(event.target.value)))
                setIncluded([])
                setIncludeNote(undefined)
              }}
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
        <MatchBuilder
          rows={rows}
          contexts={matchContexts(anchorContext, view, included)}
          eventTypes={domainEventTypes(view)}
          disabled={!ready}
          note={includeNote}
          onChange={(next) => {
            setRows(next)
            setIncludeNote(undefined)
          }}
        />
        <Button
          type="submit"
          size="sm"
          disabled={
            !ready ||
            busy ||
            !aggregateType ||
            !aggregateId.trim() ||
            !matchesValid
          }
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
              {view.truncated ? " · truncated read" : ""}
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
        {includeCandidates(view, included, matches).length > 0 && (
          <fieldset className="correlation-include" disabled={busy}>
            <legend>Load payloads from</legend>
            {includeCandidates(view, included, matches).map((context) => (
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
        {view?.matched && view.matched.length > 0 && (
          <MatchedChips
            matched={view.matched}
            disabled={busy}
            onRemove={(removed) =>
              applyMatches(
                matches.filter(
                  (match) =>
                    formatPayloadMatch(match) !== formatPayloadMatch(removed)
                )
              )
            }
            onClear={() => applyMatches([])}
          />
        )}
        {view?.matched?.length && view.correlations.length === 0 ? (
          <p className="observation-empty" role="status">
            {view.truncated
              ? "No matching correlations returned in this incomplete read."
              : "No correlation matches these filters."}{" "}
            Matching sees payloads after the Tracer payload policy; a redacting
            policy hides every field.
          </p>
        ) : null}
        {view && !view.matched?.length && view.correlations.length === 0 && (
          <p className="observation-empty" role="status">
            {view.truncated
              ? "No correlations returned in this incomplete read. More retained matches may exist."
              : "No correlations for this anchor in the retained events read."}
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
                placeholder="Read retained correlation events"
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
          <SourceCutoffs snapshot={view.snapshot} detailed />
        ) : (
          <span>Source cutoffs appear with the first result.</span>
        )}
        <span>
          Source cutoffs are captured independently: not an atomic snapshot or a
          completion signal. Later events may join the chain; expired events are
          absent.
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
  const empty = chain && chain.refs.length === 0
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
            ? "Reading retained events…"
            : chain
              ? [
                  anchor && `Anchor ${anchor.label} · ${anchor.aggregateId}`,
                  empty ? "No retained events returned" : summary?.events,
                  summary?.contexts,
                  chain.truncated && "truncated · incomplete evidence",
                ]
                  .filter(Boolean)
                  .join(" · ")
              : "Choose an anchor and its aggregate ID, then select a correlation."}
        </p>
      </div>
      {chain && (
        <div className="observation-evidence correlation-snapshot">
          <SourceCutoffs snapshot={chain.snapshot} />
          <span>
            {chain.truncated
              ? "This read reached a resource bound; more retained matches may exist."
              : "Independent source cutoffs, not a completion signal. Later events may join; expired events are absent."}
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
        This Tracer does not advertise retained correlation inspection. Check
        its Normal runtime's correlation history configuration.
      </p>
    )
  return anchorsAdvertised ? null : (
    <p className="observation-empty" role="status">
      No anchor aggregates are declared. Enter an aggregate type, or declare
      <code> ANCHOR = true</code> on an aggregate definition.
    </p>
  )
}

function SourceCutoffs({
  snapshot,
  detailed = false,
}: {
  snapshot: CorrelationSnapshot
  detailed?: boolean
}) {
  const { sources } = snapshot
  if (!sources.length) return <span>No source cutoffs returned.</span>
  return (
    <>
      <ul className="snapshot-list" aria-label="Correlation source cutoffs">
        {sources.map((source) => (
          <li
            key={`${source.context ?? ""}-${source.kind}-${source.stream}`}
            title={source.stream}
          >
            <span>
              {source.context
                ? `${source.context} · ${source.kind}`
                : `Shared ${source.kind}`}
            </span>
            <Badge variant="neutral">cutoff {source.cutoffSequence}</Badge>
          </li>
        ))}
      </ul>
      {detailed && (
        <details className="snapshot-sources">
          <summary>
            {sources.length} {sources.length === 1 ? "source" : "sources"}
          </summary>
          <ul aria-label="Correlation source streams">
            {sources.map((source) => (
              <li
                key={`${source.context ?? ""}-${source.kind}-${source.stream}`}
              >
                <span>{source.stream}</span>
                <small>
                  {source.context ?? "Shared"} · {source.kind} · cutoff{" "}
                  {source.cutoffSequence}
                </small>
              </li>
            ))}
          </ul>
        </details>
      )}
    </>
  )
}

function includeCandidates(
  view: AnchorCorrelationView | undefined,
  included: string[],
  matches: PayloadMatch[]
): string[] {
  return [
    ...new Set([
      ...(view ? payloadContexts(view) : []),
      ...included,
      ...matches.map((match) => match.context),
    ]),
  ]
    .filter(Boolean)
    .sort()
}

function matchContexts(
  anchorContext: string,
  view: AnchorCorrelationView | undefined,
  included: string[]
): string[] {
  return [
    ...new Set(
      [
        anchorContext,
        ...included,
        ...(view ? payloadContexts(view) : []),
        ...(view?.snapshot.sources.flatMap((source) =>
          source.context ? [source.context] : []
        ) ?? []),
      ].filter(Boolean)
    ),
  ]
}

function domainEventTypes(
  view: AnchorCorrelationView | undefined
): Map<string, string[]> {
  const types = new Map<string, Set<string>>()
  for (const reference of view?.correlations.flatMap((item) => item.refs) ?? [])
    if (reference.kind === "domain")
      types.set(
        reference.context,
        (types.get(reference.context) ?? new Set()).add(reference.eventType)
      )
  return new Map(
    [...types].map(([context, names]) => [context, [...names].sort()])
  )
}

function payloadContexts(view: AnchorCorrelationView): string[] {
  return [
    ...new Set(
      view.correlations.flatMap((correlation) =>
        correlation.refs
          .filter(
            (reference) =>
              reference.kind === "domain" &&
              (reference.aggregateType !== view.aggregateType ||
                reference.aggregateId !== view.aggregateId)
          )
          .map((reference) => reference.context)
      )
    ),
  ].sort()
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
      )?.eventType ?? "No retained events returned",
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
    case "not-found":
      return "No retained events"
    case "history-unavailable":
      return "Correlation history unavailable"
    case "read-failed":
      return "Correlation read failed"
    case "invalid-identifier":
      return "Invalid identifier"
    case "match-requires-include":
      return "Match needs the context's payloads"
    case "invalid-match":
      return "Invalid match expression"
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
    case "not-found":
      return `No retained events were found for ${subject}. Check the ID; this reads Normal traffic, not isolated Test runs. Expired events are absent.`
    case "history-unavailable":
      return "This Tracer cannot inspect retained correlation history. Check its Normal runtime configuration."
    case "read-failed":
      return "Tracer could not read a retained event source. This is not an empty result; retry the read."
    case "invalid-identifier":
      return "Tracer rejected the identifier. Check its spelling."
    case "match-requires-include":
      return "Tracer loads payloads for the anchor stream and included contexts only. Tick the context under Load payloads from and search again."
    case "invalid-match":
      return "Use <context>/<event-type>.<field>[.<field>…]=<value>, at most 8 matches and 8 field segments."
    case "request-failed":
      return "The request did not complete. Check the Tracer connection and retry."
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Correlation request failed"
}
