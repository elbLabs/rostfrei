import type { HistoricalMessage, HistoricalPage } from "@/lib/types"

export const MAX_TIMELINE_RECORDS = 1000
const MAX_TIMELINE_BYTES = 16 * 1024 * 1024

export function timelinePage(
  page: HistoricalPage,
  previous?: HistoricalPage
): HistoricalPage {
  if (page.order !== "time" || !page.generation)
    throw new Error("Chronological history is unavailable; refresh its catalog")
  if (previous && previous.generation !== page.generation)
    throw new Error(
      "Historical streams changed; refresh to start a new timeline"
    )
  let last = previous?.items.at(-1)
  const ids = new Set(previous?.items.map((item) => item.id))
  for (const item of page.items) {
    const timestamp = nanos(item)
    if (ids.has(item.id) || (last && timestamp < nanos(last)))
      throw new Error("Historical milestones overlap or are out of time order")
    ids.add(item.id)
    last = item
  }
  if (previous?.nextHref && previous.nextHref === page.nextHref)
    throw new Error("Historical continuation did not advance")
  const items = [...(previous?.items ?? []), ...page.items]
  if (
    items.length > MAX_TIMELINE_RECORDS ||
    new TextEncoder().encode(JSON.stringify(items)).length > MAX_TIMELINE_BYTES
  )
    throw new Error(
      "Timeline exceeds its 1,000-record / 16 MiB limit. Select a narrower aggregate or flow"
    )
  return {
    ...page,
    items,
    scannedMessages: (previous?.scannedMessages ?? 0) + page.scannedMessages,
    skippedMessages: (previous?.skippedMessages ?? 0) + page.skippedMessages,
  }
}

function nanos(item: HistoricalMessage): bigint {
  if (!item.storedAtUnixNanos || !/^-?\d{1,30}$/.test(item.storedAtUnixNanos))
    throw new Error("Historical milestone has no exact recorded timestamp")
  return BigInt(item.storedAtUnixNanos)
}

export function milestoneTime(item?: HistoricalMessage): string {
  if (!item) return "Before first milestone"
  const timestamp = nanos(item)
  const date = new Date(Number(timestamp / 1_000_000n))
  if (!Number.isFinite(date.getTime())) return item.storedAt
  const fraction = (
    ((timestamp % 1_000_000_000n) + 1_000_000_000n) %
    1_000_000_000n
  )
    .toString()
    .padStart(9, "0")
  return `${date.toISOString().slice(0, 19).replace("T", " ")}.${fraction} UTC`
}
