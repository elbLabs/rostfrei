import { advertisedHref, request } from "@/lib/api"
import type {
  HistoricalCapability,
  HistoricalPage,
  HistoricalAggregate,
  TracerCatalog,
} from "@/lib/types"

export async function productionHistory(
  signal: AbortSignal
): Promise<HistoricalCapability[]> {
  if (!import.meta.env.VITE_TRACER_INSPECTION_TOKEN) return []
  const response = await request(
    "/catalog",
    { signal: AbortSignal.any([signal, AbortSignal.timeout(12_000)]) },
    "inspection"
  )
  const catalog = (await response.json()) as TracerCatalog
  if (catalog.catalogVersion !== 1)
    throw new Error("Unsupported historical catalog version")
  return (catalog.historical ?? []).filter(
    (entry) => entry.scope === "production"
  )
}

export async function getHistoricalAggregate(
  capability: HistoricalCapability,
  params: URLSearchParams,
  signal: AbortSignal
): Promise<HistoricalAggregate> {
  if (!capability.aggregateHref)
    throw new Error("Aggregate reconstruction is unavailable")
  const href = historicalQueryHref(capability.aggregateHref, params)
  const response = await request(
    href,
    { signal, timeoutMs: 15_000 },
    capability.scope === "production" ? "inspection" : "control"
  )
  const result = (await response.json()) as HistoricalAggregate
  if (
    result.application !== capability.application ||
    result.scope !== capability.scope ||
    result.aggregateType !== params.get("aggregateType") ||
    result.aggregateId !== params.get("aggregateId") ||
    !Array.isArray(result.snapshots) ||
    !isVersion(result.version) ||
    !isVersion(result.snapshotVersion) ||
    !result.snapshots.every(
      (snapshot) =>
        isVersion(snapshot.streamVersion) && snapshot.streamVersion !== "0"
    ) ||
    typeof result.generation !== "string" ||
    (params.has("version") && result.version !== params.get("version")) ||
    (params.has("generation") && result.generation !== params.get("generation"))
  )
    throw new Error(
      "Reconstruction does not match the selected aggregate history"
    )
  return result
}

function isVersion(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^(0|[1-9][0-9]{0,19})$/.test(value) &&
    BigInt(value) <= 18446744073709551615n
  )
}

export async function getSnapshotFixture(
  capability: HistoricalCapability,
  params: URLSearchParams,
  signal: AbortSignal
): Promise<unknown> {
  if (!capability.snapshotFixtureHref)
    throw new Error("Snapshot fixture export is unavailable")
  const response = await request(
    historicalQueryHref(capability.snapshotFixtureHref, params),
    { signal, timeoutMs: 15_000 },
    capability.scope === "production" ? "inspection" : "control"
  )
  const fixture = await response.json()
  if (
    fixture.schemaVersion !== 1 ||
    !Array.isArray(fixture.messages) ||
    !Array.isArray(fixture.snapshots)
  )
    throw new Error("Tracer returned an invalid snapshot fixture")
  return fixture
}

function historicalQueryHref(href: string, params: URLSearchParams): string {
  const url = new URL(advertisedHref(href), location.origin)
  for (const [key, value] of params) url.searchParams.set(key, value)
  return url.pathname + url.search
}

export async function getHistoricalPage(
  capability: HistoricalCapability,
  href: string,
  signal: AbortSignal
): Promise<HistoricalPage> {
  const response = await request(
    advertisedHref(href),
    { signal, timeoutMs: 15_000 },
    capability.scope === "production" ? "inspection" : "control"
  )
  const page = (await response.json()) as HistoricalPage
  if (
    page.application !== capability.application ||
    page.scope !== capability.scope ||
    !Array.isArray(page.items)
  )
    throw new Error(
      "Historical page does not match the selected application and scope"
    )
  return page
}
