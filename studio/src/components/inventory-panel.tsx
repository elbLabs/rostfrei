import { useEffect, useState, type ReactNode } from "react"
import { Layers, RefreshCw } from "lucide-react"
import { Button } from "@/components/ui/button"
import { ServerHealthPanel } from "./server-health-panel"
import { serverHealthLabel, useServerHealth } from "./server-health-data"
import {
  InventoryCanvas,
  type Resource,
  type ResourceLink,
} from "./inventory-canvas"
import { WorkloadMemory } from "./workload-memory"
import { useWorkloadMemory } from "@/lib/workload-memory"
import {
  getInventory,
  inventoryErrorMessage,
  shouldRetainSnapshot,
  type BrokerConsumer,
  type BrokerStream,
  type BrokerSnapshot,
  type BrokerSource,
  type Capacity,
  type InventoryView,
  type ReadModel,
  type ReadStatus,
  type Snapshot,
} from "./inventory-panel-data"
import { Domain } from "./inventory-panel-domain"
import "./inventory-panel.css"

const copy = {
  infrastructure: {
    label: "Infrastructure inventory",
    description:
      "Usage and limits reported by configured broker sources. Reads are sampled separately and may not agree at the same instant.",
  },
  domain: {
    label: "Domain model",
    description: "Open an aggregate to explore its entities and events.",
  },
  "read-models": {
    label: "Read model inventory",
    description:
      "Discover explicitly declared projections and their storage policies. A policy does not prove the storage exists or the projection is current.",
  },
}

type Result = { revision: number } & (
  | { status: "ready"; snapshot: Snapshot }
  | { status: "error"; message: string; previous?: Snapshot }
  | { status: "unavailable" }
)

export function InventoryPanel({ view }: { view: InventoryView }) {
  return <InventoryContent key={view} view={view} />
}

function InventoryContent({ view }: { view: InventoryView }) {
  const [revision, setRevision] = useState(0)
  const [result, setResult] = useState<Result>()
  const [serversHref, setServersHref] = useState<string>()
  const state = result?.revision === revision ? result.status : "loading"
  const snapshot =
    result?.status === "ready"
      ? result.snapshot
      : result?.status === "error"
        ? result.previous
        : undefined
  const received = snapshot?.received
  const { label, description } = copy[view]

  useEffect(() => {
    const controller = new AbortController()
    // A mount discarded before the next microtask does not need a broker sample.
    queueMicrotask(() => {
      if (controller.signal.aborted) return
      let discoveredServerLink = false
      void getInventory(
        view,
        controller.signal,
        view === "infrastructure"
          ? (href) => {
              if (controller.signal.aborted) return
              discoveredServerLink = true
              setServersHref(href)
            }
          : undefined
      ).then(
        (snapshot) => {
          if (controller.signal.aborted) return
          setResult(
            snapshot
              ? { revision, status: "ready", snapshot }
              : { revision, status: "unavailable" }
          )
        },
        (error: unknown) => {
          if (controller.signal.aborted) return
          if (
            view === "infrastructure" &&
            !discoveredServerLink &&
            !shouldRetainSnapshot(error)
          )
            setServersHref(undefined)
          setResult((previous) => ({
            revision,
            status: "error",
            message: inventoryErrorMessage(error),
            // Permission loss must not leave previously authorized evidence on screen.
            previous: shouldRetainSnapshot(error)
              ? previous?.status === "ready"
                ? previous.snapshot
                : previous?.status === "error"
                  ? previous.previous
                  : undefined
              : undefined,
          }))
        }
      )
    })
    return () => controller.abort()
  }, [revision, view])

  const domainView = view === "domain"
  const refresh = (
    <Button
      variant="outline"
      size="icon"
      aria-label="Refresh snapshot"
      title={
        received
          ? `Last received ${formatDate(received)}. Refresh snapshot`
          : "Refresh snapshot"
      }
      disabled={state === "loading"}
      onClick={() => setRevision((value) => value + 1)}
    >
      <RefreshCw aria-hidden="true" />
    </Button>
  )
  const notices = (
    <>
      <div role="status">
        {state === "loading" && (
          <Notice>
            {snapshot
              ? "Refreshing… Showing the previous snapshot."
              : "Loading inventory…"}
          </Notice>
        )}
        {state === "unavailable" && (
          <Notice>
            {view === "infrastructure"
              ? "JetStream account inventory is not advertised by Tracer for inspection."
              : "This inventory is not advertised by Tracer for inspection."}
          </Notice>
        )}
      </div>
      {state === "error" && result?.status === "error" && (
        <Notice alert>
          <strong>Could not load inventory.</strong> {result.message}{" "}
          {snapshot && "Showing the previous snapshot."}
        </Notice>
      )}
    </>
  )

  if (!domainView) {
    const common = {
      actions: refresh,
      notices: (
        <>
          {notices}
          {received && <SampleTime received={received} />}
        </>
      ),
    }
    return (
      <section
        className="inventory-panel inventory-resource-workspace"
        aria-label={label}
        data-inventory-view={view}
        data-inventory-state={state}
      >
        {view === "infrastructure" ? (
          <Infrastructure
            {...common}
            data={
              snapshot?.kind === "infrastructure" ? snapshot.data : undefined
            }
            serversHref={serversHref}
          />
        ) : (
          <ReadModels
            {...common}
            snapshot={snapshot?.kind === "domain" ? snapshot : undefined}
          />
        )}
      </section>
    )
  }

  return (
    <section
      className="domain-workspace"
      aria-label={label}
      data-inventory-view={view}
      data-inventory-state={state}
    >
      <div className="domain-sheet">
        {!snapshot && (
          <header className="workspace-toolbar">
            <div className="workspace-identity">
              <span className="workspace-icon">
                <Layers size={21} aria-hidden="true" />
              </span>
              <div>
                <div className="workspace-title">
                  <h1>{label}</h1>
                </div>
                <p>{description}</p>
              </div>
            </div>
            {refresh}
          </header>
        )}
        {notices}
        {snapshot?.kind === "domain" && (
          <Domain data={snapshot.data} refresh={refresh} />
        )}
      </div>
    </section>
  )
}

function formatDate(date: Date) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "long",
  }).format(date)
}

function SampleTime({ received }: { received: Date }) {
  return (
    <p className="inventory-timestamp">
      Received by Studio{" "}
      <time dateTime={received.toISOString()}>{formatDate(received)}</time>
    </p>
  )
}

const statusText = {
  complete: "Complete",
  truncated: "Truncated",
  timedOut: "Timed out",
  unavailable: "Unavailable",
}

function Status({ children, tone }: { children: ReactNode; tone?: "warning" }) {
  return (
    <span className="inventory-status" data-tone={tone}>
      {children}
    </span>
  )
}

function Heading({
  title,
  status,
  children,
}: {
  title: string
  status?: ReadStatus
  children?: ReactNode
}) {
  return (
    <div className="inventory-section-heading">
      <h3>{title}</h3>
      {status && status !== "complete" && (
        <Status tone="warning">{statusText[status]}</Status>
      )}
      {children}
    </div>
  )
}

function Notice({ children, alert }: { children: ReactNode; alert?: boolean }) {
  return (
    <p className="inventory-notice" role={alert ? "alert" : undefined}>
      {children}
    </p>
  )
}

function Empty({ status, subject }: { status: ReadStatus; subject: string }) {
  return (
    <p className="inventory-muted">
      {status === "complete"
        ? `No ${subject} reported.`
        : `No ${subject} returned in this incomplete read.`}
    </p>
  )
}

function Facts({ values }: { values: [string, ReactNode][] }) {
  return (
    <dl className="inventory-facts">
      {values.map(([label, value]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  )
}

const numberFormat = new Intl.NumberFormat(undefined, {
  maximumFractionDigits: 2,
})

function bytes(value: number) {
  if (value < 1024) return `${numberFormat.format(value)} B`
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"]
  const index = Math.min(
    units.length - 1,
    Math.floor(Math.log(value) / Math.log(1024))
  )
  return `${numberFormat.format(value / 1024 ** index)} ${units[index]}`
}

function Budget({ label, capacity }: { label: string; capacity: Capacity }) {
  const { usedBytes, limitBytes } = capacity
  const percent =
    limitBytes !== null && limitBytes > 0
      ? (usedBytes / limitBytes) * 100
      : undefined
  return (
    <div className="inventory-budget">
      <div className="inventory-budget-caption">
        <strong>{label}</strong>
        <span>
          {bytes(usedBytes)} used ·{" "}
          {limitBytes === null
            ? "No finite limit reported"
            : `${bytes(limitBytes)} limit`}
        </span>
      </div>
      {percent !== undefined && (
        <div
          className="inventory-budget-track"
          aria-hidden="true"
          data-at-limit={percent >= 100}
        >
          <span style={{ width: `${Math.min(percent, 100)}%` }} />
        </div>
      )}
    </div>
  )
}

const connectionLabels = {
  connecting: "Client connecting",
  connected: "Client connected",
  disconnected: "Client disconnected",
  closed: "Client closed",
  timedOut: "Connection timed out",
  unavailable: "Connection unavailable",
}
const storageLabels = { file: "File storage", memory: "Memory storage" }

const resourceId = (kind: string, ...parts: (string | number | undefined)[]) =>
  JSON.stringify([kind, ...parts])
type CanvasActions = { actions: ReactNode; notices: ReactNode }

function Infrastructure({
  data,
  serversHref,
  actions,
  notices,
}: CanvasActions & {
  data?: BrokerSnapshot
  serversHref?: string
}) {
  const [path, navigate] = useState<{ source: string; stream?: string }>()
  const monitoring = useServerHealth(serversHref)
  const sources = data?.sources ?? []
  const source = sources.find(({ label }) => label === path?.source)
  const stream = source?.streams.items.find(({ name }) => name === path?.stream)
  const sourceCard = (source: BrokerSource, drill: boolean): Resource => ({
    id: resourceId("source", source.label),
    kind: "source",
    title: source.label,
    caption: `${source.streams.items.length} streams · ${source.kvBuckets.items.length} KV buckets returned`,
    summary: connectionLabels[source.connection],
    warning: source.connection !== "connected",
    drill,
    details: <Source source={source} />,
  })
  const streamCard = (
    source: BrokerSource,
    stream: BrokerStream,
    drill: boolean
  ): Resource => ({
    id: resourceId("stream", source.label, stream.name),
    kind: "stream",
    title: stream.name,
    caption: `${storageLabels[stream.storage]} · ${bytes(stream.capacity.usedBytes)} retained`,
    summary: `${stream.consumers.items.length} consumers returned${stream.consumers.status === "complete" ? "" : ` · ${statusText[stream.consumers.status]}`}`,
    warning: stream.consumers.status !== "complete",
    drill,
    details: <StreamDetails stream={stream} />,
  })

  let columns: Resource[][]
  const links: ResourceLink[] = []
  let scopeNotice: ReactNode
  const result = monitoring?.result
  const reading = monitoring?.reading
  const servers: Resource[] = monitoring
    ? [
        ...(reading?.servers.length
          ? reading.servers.map((server): Resource => ({
              id: resourceId("server", server.label),
              kind: "server",
              title: server.label,
              caption:
                server.metrics.status === "available" &&
                server.metrics.value.residentMemoryBytes !== undefined
                  ? `${bytes(server.metrics.value.residentMemoryBytes)} process RAM`
                  : undefined,
              summary: `${serverHealthLabel(server)}${monitoring.loading || result?.status === "error" ? " · Previous sample" : ""}`,
              warning:
                server.health.status !== "available" ||
                server.health.value !== "healthy",
              details: (
                <ServerHealthPanel sample={monitoring} label={server.label} />
              ),
            }))
          : [
              {
                id: resourceId("monitoring"),
                kind: "monitoring" as const,
                title: "Configured NATS nodes",
                summary: monitoring.loading
                  ? "Sampling server nodes…"
                  : result?.status === "error"
                    ? "Unavailable"
                    : "No nodes reported",
                warning: result?.status === "error",
                details: <ServerHealthPanel sample={monitoring} />,
              },
            ]),
      ]
    : []

  if (source && stream) {
    const parent = streamCard(source, stream, false)
    const children = stream.consumers.items.map((consumer): Resource => ({
      id: resourceId("consumer", source.label, stream.name, consumer.name),
      kind: "consumer",
      title: consumer.name,
      caption: `${consumer.kind === "pull" ? "Pull" : "Push"} · ${consumer.durable ? "Durable" : "Ephemeral"} · ${numberFormat.format(consumer.pending)} pending`,
      summary: consumer.paused ? "Paused" : liveness[consumer.liveness],
      warning: consumer.paused,
      details: <Consumer data={consumer} />,
    }))
    columns = [[parent], children]
    links.push(
      ...children.map(({ id }): ResourceLink => ({
        source: parent.id,
        target: id,
        kind: "reports-consumer",
      }))
    )
    if (!children.length)
      scopeNotice = (
        <Empty subject="consumers" status={stream.consumers.status} />
      )
  } else if (source) {
    const parent = sourceCard(source, false)
    const streams = source.streams.items.map((stream) =>
      streamCard(source, stream, true)
    )
    const buckets = source.kvBuckets.items.map((bucket): Resource => ({
      id: resourceId("bucket", source.label, bucket.name),
      kind: "bucket",
      title: bucket.name,
      caption: bucket.stream,
      summary: `${bytes(bucket.capacity.usedBytes)} · ${storageLabels[bucket.storage]}`,
      details: (
        <>
          <Budget label="Backing stream bytes" capacity={bucket.capacity} />
          <Facts
            values={[
              ["Backing stream", <code>{bucket.stream}</code>],
              ["Medium", storageLabels[bucket.storage]],
            ]}
          />
        </>
      ),
    }))
    columns = [[parent], streams, buckets]
    links.push(
      ...streams.map(({ id }): ResourceLink => ({
        source: parent.id,
        target: id,
        kind: "reports-stream",
      }))
    )
    for (const [index, bucket] of source.kvBuckets.items.entries()) {
      const backing = streams.find(({ title }) => title === bucket.stream)
      // A KV record names its backing stream. If that stream was not returned,
      // the source still reported this bucket, but there is no stream card to link.
      links.push(
        backing
          ? {
              source: backing.id,
              target: buckets[index].id,
              kind: "backs-bucket",
            }
          : {
              source: parent.id,
              target: buckets[index].id,
              kind: "reports-bucket",
            }
      )
    }
    scopeNotice = (
      <p className="inventory-scope-summary">
        Streams: {statusText[source.streams.status]} · KV:{" "}
        {statusText[source.kvBuckets.status]}
      </p>
    )
  } else {
    columns = [sources.map((source) => sourceCard(source, true)), servers]
    if (data && !sources.length)
      scopeNotice = <Notice>No broker sources reported.</Notice>
  }

  const scope = stream
    ? resourceId("stream", source?.label, stream.name)
    : source
      ? resourceId("source", source.label)
      : undefined
  return (
    <InventoryCanvas
      label="Infrastructure canvas"
      title={
        stream
          ? "Stream resources"
          : source
            ? "Broker resources"
            : "Infrastructure"
      }
      columns={columns}
      links={links}
      scope={scope}
      back={
        source && {
          label: stream ? "Back to broker source" : "All sources",
          action: () => navigate(stream ? { source: source.label } : undefined),
        }
      }
      drill={(id) => {
        if (source) {
          const stream = source.streams.items.find(
            ({ name }) => id === resourceId("stream", source.label, name)
          )
          if (stream) navigate({ source: source.label, stream: stream.name })
        } else {
          const match = sources.find(
            ({ label }) => id === resourceId("source", label)
          )
          if (match) navigate({ source: match.label })
        }
      }}
      actions={actions}
      notices={
        <>
          {notices}
          {scopeNotice}
        </>
      }
    />
  )
}

function Source({ source }: { source: BrokerSource }) {
  const { label, connection, account, streams, kvBuckets } = source
  return (
    <>
      <p className="inventory-muted">
        Usage and limits from this configured broker source. Reads are sampled
        separately and may not agree at the same instant.
      </p>
      <header className="inventory-source-heading">
        <strong>{label}</strong>
        <Status tone={connection === "connected" ? undefined : "warning"}>
          {connectionLabels[connection]}
        </Status>
      </header>
      <section className="inventory-section">
        <Heading title="JetStream account" status={account.status} />
        {account.usage ? (
          <>
            <div className="inventory-budget-grid">
              <Budget label="Memory" capacity={account.usage.memory} />
              <Budget label="File storage" capacity={account.usage.storage} />
            </div>
            <Facts
              values={[
                ["Streams", numberFormat.format(account.usage.streamCount)],
                ["Consumers", numberFormat.format(account.usage.consumerCount)],
              ]}
            />
            {account.usage.tiers.length > 0 && (
              <details className="inventory-details">
                <summary>
                  Replica-tier budgets <span>{account.usage.tiers.length}</span>
                </summary>
                <div className="inventory-details-body">
                  <p className="inventory-muted">
                    These quotas apply to their own replica class and are not
                    additive.
                  </p>
                  {account.usage.tiers.map((tier) => (
                    <div key={tier.name} className="inventory-item">
                      <h4>{tier.name}</h4>
                      <div className="inventory-budget-grid">
                        <Budget label="Memory" capacity={tier.memory} />
                        <Budget label="File storage" capacity={tier.storage} />
                      </div>
                    </div>
                  ))}
                </div>
              </details>
            )}
          </>
        ) : (
          <p className="inventory-muted">Account usage not reported.</p>
        )}
      </section>
      <section className="inventory-section">
        <Heading title="Streams" status={streams.status}>
          <span className="inventory-count">
            {streams.items.length} returned
          </span>
        </Heading>
        {!streams.items.length && (
          <Empty status={streams.status} subject="streams" />
        )}
      </section>
      <section className="inventory-section">
        <Heading title="KV buckets" status={kvBuckets.status}>
          <span className="inventory-count">
            {kvBuckets.items.length} returned
          </span>
        </Heading>
        {!kvBuckets.items.length && (
          <Empty status={kvBuckets.status} subject="KV buckets" />
        )}
      </section>
    </>
  )
}

const liveness = {
  pushBound: "Push subscription bound",
  pushUnbound: "No push subscription bound",
  pullWaiting: "Pull request waiting",
  unknown: "Liveness unknown",
}

function StreamDetails({ stream }: { stream: BrokerStream }) {
  const { items, status } = stream.consumers
  return (
    <>
      <Budget label="Retained bytes" capacity={stream.capacity} />
      <Facts
        values={[
          ["Retained messages", numberFormat.format(stream.messages)],
          ["Medium", storageLabels[stream.storage]],
          ["Replicas", stream.replicas],
        ]}
      />
      <Heading title="Consumers" status={status}>
        <span>
          {items.length} returned · {numberFormat.format(stream.consumerCount)}{" "}
          reported
        </span>
      </Heading>
      <ConsumerCaveat />
      {!items.length && <Empty status={status} subject="consumers" />}
    </>
  )
}

function ConsumerCaveat() {
  return (
    <p className="inventory-muted">
      Broker signals at sample time, not proof of worker health. Sequence
      positions are not backlog counts.
    </p>
  )
}

function Consumer({ data }: { data: BrokerConsumer }) {
  return (
    <article className="inventory-item">
      <div className="inventory-item-heading">
        <span>
          {data.kind === "pull" ? "Pull" : "Push"} ·{" "}
          {data.durable ? "Durable" : "Ephemeral"}
        </span>
      </div>
      <div className="inventory-tags">
        <Status>{liveness[data.liveness]}</Status>
        {data.paused && <Status tone="warning">Paused</Status>}
      </div>
      <Facts
        values={[
          ["Pending delivery", numberFormat.format(data.pending)],
          ["Awaiting acknowledgment", numberFormat.format(data.ackPending)],
          ["Redelivered", numberFormat.format(data.redelivered)],
          ["Waiting pulls", numberFormat.format(data.waiting)],
          ["Delivered stream sequence", data.deliveredStreamSequence],
          [
            "Acknowledgment floor · stream sequence",
            data.ackFloorStreamSequence,
          ],
        ]}
      />
      <ConsumerCaveat />
    </article>
  )
}

type BrokerSample = { data: BrokerSnapshot; received: Date }

function ReadModels({
  snapshot,
  actions,
  notices,
}: CanvasActions & {
  snapshot?: Extract<Snapshot, { kind: "domain" }>
}) {
  const models = snapshot?.data.readModels ?? []
  const workloadSample = useWorkloadMemory(
    models.some(({ workload }) => workload)
      ? snapshot?.workloadsHref
      : undefined,
    snapshot?.received
  )
  const stores = new Map<string, Resource>()
  const links: ResourceLink[] = []
  const modelCards = models.map((model): Resource => {
    const storage = model.storage?.kind === "natsKv" ? model.storage : undefined
    const id = resourceId(
      "model",
      model.context,
      model.name,
      model.schemaVersion,
      storage?.application,
      storage?.scope,
      storage?.bucketName,
      model.brokerSource
    )
    if (storage) {
      const storeId = resourceId(
        "store",
        storage.application,
        storage.scope,
        storage.bucketName,
        storage.streamName,
        model.brokerSource ?? id
      )
      links.push({ source: id, target: storeId, kind: "declares-store" })
      if (!stores.has(storeId))
        stores.set(storeId, {
          id: storeId,
          kind: "store",
          title: storage.bucketName,
          caption: storage.streamName,
          summary: model.brokerSource ?? "Broker source not declared",
          details: (
            <>
              <p className="inventory-muted">
                Declared storage is not proof that the store exists or that the
                projection is current.
              </p>
              <Facts
                values={[
                  ["Application", <code>{storage.application}</code>],
                  [
                    "Scope",
                    storage.scope === "production" ? "Production" : "Test",
                  ],
                  ["Backing stream", <code>{storage.streamName}</code>],
                  ["Broker source", model.brokerSource ?? "Not declared"],
                ]}
              />
              {snapshot?.broker && model.brokerSource ? (
                <ReadModelUsage model={model} broker={snapshot.broker} />
              ) : (
                <p className="inventory-muted">
                  {model.brokerSource
                    ? "Broker usage unavailable."
                    : "No broker source is declared for sampling this store."}
                </p>
              )}
            </>
          ),
        })
    }
    return {
      id,
      kind: "model",
      title: model.name,
      caption: model.context,
      summary: storage
        ? `Schema v${model.schemaVersion}`
        : `Schema v${model.schemaVersion} · Storage not declared`,
      details: (
        <div className="inventory-item">
          <div className="inventory-item-heading">
            <span>Schema v{model.schemaVersion}</span>
          </div>
          <Facts values={[["Bounded context", <code>{model.context}</code>]]} />
          {model.workload && (
            <WorkloadMemory
              label={model.workload}
              state={workloadSample ?? { status: "unavailable" }}
            />
          )}
          {storage ? (
            <div className="inventory-mapping">
              <h4>
                Declared JetStream KV policy{" "}
                <Status>
                  {storage.scope === "production" ? "Production" : "Test"}
                </Status>
              </h4>
              <Facts
                values={[
                  ["Application", <code>{storage.application}</code>],
                  ["KV bucket", <code>{storage.bucketName}</code>],
                  ["Backing stream", <code>{storage.streamName}</code>],
                  ["Medium", storageLabels[storage.storage]],
                  [
                    "Broker source",
                    model.brokerSource ? (
                      <code>{model.brokerSource}</code>
                    ) : (
                      "Not declared"
                    ),
                  ],
                  ["Max bytes", `${storage.maxBytes} B`],
                  ["Max value bytes", `${storage.maxValueBytes} B`],
                  ["Replicas", storage.replicas],
                  ["History per key", storage.history],
                  [
                    "TTL",
                    storage.ttlNanos === "0"
                      ? "No TTL"
                      : `${storage.ttlNanos} ns`,
                  ],
                ]}
              />
            </div>
          ) : (
            <p className="inventory-muted">Storage mapping not declared.</p>
          )}
        </div>
      ),
    }
  })
  return (
    <InventoryCanvas
      label="Read models canvas"
      title="Read models"
      columns={[modelCards, [...stores.values()]]}
      links={links}
      actions={actions}
      notices={
        <>
          {notices}
          {snapshot && !models.length && (
            <Notice>No read models declared in this inventory.</Notice>
          )}
          {snapshot?.brokerUnavailable && (
            <Notice>
              Broker inventory unavailable. Declared policies are shown without
              usage. {snapshot.brokerError}
            </Notice>
          )}
        </>
      }
    />
  )
}

function ReadModelUsage({
  model,
  broker,
}: {
  model: ReadModel
  broker: BrokerSample
}) {
  const sources = broker.data.sources.filter(
    ({ label }) => label === model.brokerSource
  )
  const source = sources.length === 1 ? sources[0] : undefined
  const bucket = source?.kvBuckets.items.find(
    ({ name, stream }) =>
      name === model.storage?.bucketName && stream === model.storage?.streamName
  )
  if (!bucket)
    return (
      <p className="inventory-muted">
        No matching bucket in the broker sample
        {source && source.kvBuckets.status !== "complete"
          ? ` (${statusText[source.kvBuckets.status].toLowerCase()})`
          : ""}
        . This does not establish absence.
      </p>
    )
  return (
    <div className="inventory-usage">
      <Budget label="Sampled backing stream bytes" capacity={bucket.capacity} />
      <p className="inventory-timestamp">
        Broker sample received{" "}
        <time dateTime={broker.received.toISOString()}>
          {formatDate(broker.received)}
        </time>
        {source &&
          source.kvBuckets.status !== "complete" &&
          ` · ${statusText[source.kvBuckets.status]} bucket listing`}
      </p>
    </div>
  )
}
