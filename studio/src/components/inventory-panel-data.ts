import { advertisedHref, request, TracerResponseError } from "@/lib/api"

export type InventoryView = "infrastructure" | "domain" | "read-models"
export type ReadStatus = "complete" | "truncated" | "timedOut" | "unavailable"

export interface Inventory<T> {
  status: ReadStatus
  items: T[]
}

export interface Capacity {
  usedBytes: number
  limitBytes: number | null
}

export interface BrokerConsumer {
  name: string
  kind: "pull" | "push"
  durable: boolean
  paused: boolean
  liveness: "pushBound" | "pushUnbound" | "pullWaiting" | "unknown"
  pending: number
  ackPending: number
  redelivered: number
  waiting: number
  deliveredStreamSequence: string
  ackFloorStreamSequence: string
}

export interface BrokerStream {
  name: string
  storage: "memory" | "file"
  replicas: number
  messages: number
  capacity: Capacity
  consumerCount: number
  consumers: Inventory<BrokerConsumer>
}

export interface BrokerSource {
  label: string
  connection:
    | "connecting"
    | "connected"
    | "disconnected"
    | "closed"
    | "timedOut"
    | "unavailable"
  account: {
    status: ReadStatus
    usage: {
      memory: Capacity
      storage: Capacity
      streamCount: number
      consumerCount: number
      tiers: { name: string; memory: Capacity; storage: Capacity }[]
    } | null
  }
  streams: Inventory<BrokerStream>
  kvBuckets: Inventory<{
    name: string
    stream: string
    storage: BrokerStream["storage"]
    capacity: Capacity
  }>
}

export interface BrokerSnapshot {
  sources: BrokerSource[]
}

export interface AggregateId {
  context: string
  local: string
}

export interface EntityId {
  aggregate: AggregateId
  local: string
}

export interface DomainInventory {
  compiledModelAvailable: boolean
  boundedContexts: { id: string; label?: string }[]
  aggregates: {
    id: AggregateId
    label?: string
    root?: EntityId
    anchor: boolean
    aggregateType?: string
  }[]
  entities: { id: EntityId; label?: string }[]
  domainIdentities: unknown[]
  valueObjects: unknown[]
  domainServices: unknown[]
  domainEvents: unknown[]
  domainErrors: unknown[]
  actions: unknown[]
  policies: unknown[]
  queries: unknown[]
  invariants: unknown[]
  registeredCommands: unknown[]
  registeredQueries: unknown[]
}

export interface ReadModel {
  workload?: string
  context: string
  name: string
  schemaVersion: number
  brokerSource?: string
  storage?: {
    kind: "natsKv"
    application: string
    scope: "production" | "test"
    bucketName: string
    streamName: string
    maxBytes: string
    maxValueBytes: number
    storage: BrokerStream["storage"]
    replicas: number
    history: number
    ttlNanos: string
  }
}

export interface DomainSnapshot extends DomainInventory {
  readModels: ReadModel[]
}

interface InspectionCatalog {
  catalogVersion: number
  infrastructure?: { snapshotHref: string }
  domainInventory?: { snapshotHref: string }
  workloads?: { snapshotHref: string }
  servers?: { snapshotHref: string }
}

export type Snapshot =
  | { kind: "infrastructure"; data: BrokerSnapshot; received: Date }
  | {
      kind: "domain"
      data: DomainSnapshot
      received: Date
      broker?: { data: BrokerSnapshot; received: Date }
      brokerUnavailable?: boolean
      brokerError?: string
      workloadsHref?: string
    }

export function shouldRetainSnapshot(error: unknown) {
  return (
    !(error instanceof TracerResponseError) ||
    ![401, 403, 404, 410].includes(error.status)
  )
}

export function inventoryErrorMessage(error: unknown) {
  if (error instanceof TracerResponseError && error.status === 401) {
    return "Inspection authentication failed. Check VITE_TRACER_INSPECTION_TOKEN."
  }
  if (error instanceof SyntaxError)
    return "Tracer returned an invalid inventory response."
  return error instanceof Error ? error.message : "Inventory request failed."
}

async function followLink(href: string, signal: AbortSignal) {
  if (typeof href !== "string")
    throw new Error("Tracer did not advertise a valid inventory link")
  return request(advertisedHref(href), { signal }, "inspection")
}

async function brokerInventory(href: string, signal: AbortSignal) {
  const data = (await (await followLink(href, signal)).json()) as BrokerSnapshot
  if (!Array.isArray(data?.sources))
    throw new Error("Tracer did not return a broker inventory snapshot")
  return { data, received: new Date() }
}

export async function getInventory(
  view: InventoryView,
  signal: AbortSignal,
  onServerLink?: (href: string | undefined) => void
): Promise<Snapshot | undefined> {
  const catalog = (await (
    await request("/catalog", { signal }, "inspection")
  ).json()) as InspectionCatalog
  if (catalog?.catalogVersion !== 1)
    throw new Error("Unsupported Tracer catalog version")
  onServerLink?.(catalog.servers?.snapshotHref)
  const link =
    view === "infrastructure" ? catalog.infrastructure : catalog.domainInventory
  if (!link) return undefined
  if (view === "infrastructure") {
    return {
      kind: "infrastructure",
      ...(await brokerInventory(link.snapshotHref, signal)),
    }
  }
  const data = (await (
    await followLink(link.snapshotHref, signal)
  ).json()) as DomainSnapshot
  if (
    !Array.isArray(data?.boundedContexts) ||
    !Array.isArray(data?.aggregates) ||
    !Array.isArray(data?.entities) ||
    !Array.isArray(data?.readModels)
  ) {
    throw new Error("Tracer did not return a domain inventory snapshot")
  }
  const snapshot = {
    kind: "domain",
    data,
    received: new Date(),
    workloadsHref: catalog.workloads?.snapshotHref,
  } as const
  if (
    view !== "read-models" ||
    !data.readModels.some(
      (model) => model.brokerSource && model.storage?.kind === "natsKv"
    )
  )
    return snapshot
  if (!catalog.infrastructure) return { ...snapshot, brokerUnavailable: true }
  try {
    const broker = await brokerInventory(
      catalog.infrastructure.snapshotHref,
      signal
    )
    return { ...snapshot, broker }
  } catch (error) {
    return {
      ...snapshot,
      brokerUnavailable: true,
      brokerError: inventoryErrorMessage(error),
    }
  }
}
