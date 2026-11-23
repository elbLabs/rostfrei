import { useEffect, useState } from "react"
import { advertisedHref, requestJson } from "@/lib/api"

type Memory = {
  currentBytes: number | null
  limit:
    | { status: "limited"; bytes: number }
    | { status: "unlimited" | "unavailable" }
}

export type WorkloadMemoryState =
  | { status: "loading" | "unavailable" }
  | { status: "ready"; byLabel: ReadonlyMap<string, Memory> }

export function useWorkloadMemory(
  snapshotHref?: string,
  inventoryReceived?: Date
) {
  const [result, setResult] = useState<{
    href: string
    inventoryReceived?: Date
    state: WorkloadMemoryState
  }>()

  useEffect(() => {
    if (!snapshotHref) return
    const controller = new AbortController()
    const publish = (state: WorkloadMemoryState) => {
      if (!controller.signal.aborted)
        setResult({ href: snapshotHref, inventoryReceived, state })
    }
    queueMicrotask(() => {
      if (controller.signal.aborted) return
      void (async () => {
        try {
          const snapshot = await requestJson<{
            workloads: { label: string; memory: Memory }[]
          }>(
            advertisedHref(snapshotHref),
            { signal: controller.signal },
            "inspection"
          )
          publish({
            status: "ready",
            byLabel: new Map(
              snapshot.workloads.map(({ label, memory }) => [label, memory])
            ),
          })
        } catch {
          publish({ status: "unavailable" })
        }
      })()
    })
    return () => controller.abort()
  }, [snapshotHref, inventoryReceived])

  const state: WorkloadMemoryState | undefined = !snapshotHref
    ? undefined
    : result?.href === snapshotHref &&
        result.inventoryReceived === inventoryReceived
      ? result.state
      : { status: "loading" }
  return state
}
