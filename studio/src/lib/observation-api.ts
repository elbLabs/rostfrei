import { advertisedHref, request } from "@/lib/api"
import type {
  ObservationCapability,
  ObservationSnapshot,
  ObservedFlow,
  TracerCatalog,
} from "@/lib/types"

export async function productionObservation(
  signal: AbortSignal
): Promise<ObservationCapability[]> {
  if (!import.meta.env.VITE_TRACER_INSPECTION_TOKEN) return []
  const response = await request(
    "/catalog",
    { signal: AbortSignal.any([signal, AbortSignal.timeout(12_000)]) },
    "inspection"
  )
  const catalog = (await response.json()) as TracerCatalog
  if (catalog.catalogVersion !== 1)
    throw new Error("Unsupported observation catalog version")
  return (catalog.observation ?? []).filter(
    (entry) => entry.scope === "production"
  )
}

export async function getObservedFlow(
  scope: ObservationCapability["scope"],
  href: string,
  signal: AbortSignal
): Promise<ObservedFlow> {
  const response = await request(
    advertisedHref(href),
    {
      signal: AbortSignal.any([signal, AbortSignal.timeout(12_000)]),
    },
    scope === "production" ? "inspection" : "control"
  )
  return (await response.json()) as ObservedFlow
}

// Native EventSource cannot send our bearer header. Use fetch streaming with
// cancellation, bounded frames, and an idle watchdog that includes keep-alives.
export async function followObservation(
  capability: ObservationCapability,
  signal: AbortSignal,
  onSnapshot: (snapshot: ObservationSnapshot) => void
): Promise<void> {
  const watchdog = new AbortController()
  let timer = window.setTimeout(() => watchdog.abort(), 35_000)
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined
  try {
    const response = await request(
      advertisedHref(capability.eventsHref),
      {
        signal: AbortSignal.any([signal, watchdog.signal]),
        headers: { accept: "text/event-stream" },
      },
      capability.scope === "production" ? "inspection" : "control",
      true
    )
    if (
      !response.headers.get("content-type")?.includes("text/event-stream") ||
      !response.body
    ) {
      throw new Error("Tracer did not return an observation stream")
    }
    reader = response.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ""
    while (!signal.aborted) {
      const { done, value } = await reader.read()
      if (done) throw new Error("Observation connection closed; reconnecting")
      window.clearTimeout(timer)
      timer = window.setTimeout(() => watchdog.abort(), 35_000)
      buffer += decoder.decode(value, { stream: true })
      if (buffer.length > 1_048_576)
        throw new Error("Observation frame exceeds its size limit")
      let separator: RegExpExecArray | null
      while ((separator = /\r?\n\r?\n/.exec(buffer))) {
        const frame = buffer.slice(0, separator.index)
        buffer = buffer.slice(separator.index + separator[0].length)
        const lines = frame.split(/\r?\n/)
        if (!lines.some((line) => line === "event: observation")) continue
        const data = lines
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n")
        const snapshot = JSON.parse(data) as ObservationSnapshot
        if (
          snapshot.scope !== capability.scope ||
          snapshot.application !== capability.application ||
          !Array.isArray(snapshot.items)
        ) {
          throw new Error("Observation scope does not match discovery")
        }
        onSnapshot(snapshot)
      }
    }
  } finally {
    window.clearTimeout(timer)
    await reader?.cancel().catch(() => undefined)
    reader?.releaseLock()
  }
}
