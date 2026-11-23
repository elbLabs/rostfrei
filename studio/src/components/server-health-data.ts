import { useEffect, useState } from "react"
import { advertisedHref, request } from "@/lib/api"
import {
  inventoryErrorMessage,
  shouldRetainSnapshot,
} from "./inventory-panel-data"

type Failure =
  | "inaccessible"
  | "timedOut"
  | "unexpectedHttpStatus"
  | "bodyTooLarge"
  | "invalidResponse"
type Probe<T> =
  { status: "available"; value: T } | { status: "unavailable"; reason: Failure }
export type ServerSample = {
  label: string
  health: Probe<"healthy" | "unhealthy">
  metrics: Probe<{
    residentMemoryBytes?: number
    version?: string
    uptime?: string
    jetStreamLimits?: { maxMemoryBytes?: number; maxStorageBytes?: number }
  }>
}

export const failureLabel: Record<Failure, string> = {
  inaccessible: "Inaccessible",
  timedOut: "Timed out",
  unexpectedHttpStatus: "Unexpected HTTP response",
  bodyTooLarge: "Response too large",
  invalidResponse: "Invalid monitoring response",
}

export function serverHealthLabel({ health }: ServerSample) {
  return health.status === "unavailable"
    ? `Health: ${failureLabel[health.reason]}`
    : health.value === "healthy"
      ? "Healthy"
      : "Unhealthy"
}

type ServerReading = { servers: ServerSample[]; received: Date }
type Result =
  | { status: "error"; message: string; retained?: ServerReading }
  | ({ status: "ready" } & ServerReading)

async function load(href: string, signal: AbortSignal): Promise<ServerReading> {
  const response = await request(
    advertisedHref(href),
    { signal, timeoutMs: 20_000 },
    "inspection"
  )
  const snapshot = (await response.json()) as { servers?: ServerSample[] }
  if (!Array.isArray(snapshot.servers))
    throw new Error("Invalid monitoring response.")
  return { servers: snapshot.servers, received: new Date() }
}

export function useServerHealth(href?: string) {
  const [revision, setRevision] = useState(0)
  const [sample, setSample] = useState<{
    href: string
    revision: number
    result: Result
  }>()
  const result = sample?.href === href ? sample?.result : undefined
  const reading = result?.status === "ready" ? result : result?.retained
  const loading = sample?.href !== href || sample?.revision !== revision

  useEffect(() => {
    if (!href) return
    const controller = new AbortController()
    queueMicrotask(() => {
      if (controller.signal.aborted) return
      void load(href, controller.signal).then(
        (snapshot) => {
          if (!controller.signal.aborted)
            setSample({
              href,
              revision,
              result: { status: "ready", ...snapshot },
            })
        },
        (error: unknown) => {
          if (controller.signal.aborted) return
          setSample((previous) => {
            const precedent =
              previous?.href === href && shouldRetainSnapshot(error)
                ? previous.result
                : undefined
            return {
              href,
              revision,
              result: {
                status: "error",
                message: inventoryErrorMessage(error),
                retained:
                  precedent?.status === "ready"
                    ? precedent
                    : precedent?.retained,
              },
            }
          })
        }
      )
    })
    return () => controller.abort()
  }, [href, revision])

  return href
    ? {
        result,
        reading,
        loading,
        refresh: () => setRevision((value) => value + 1),
      }
    : undefined
}
