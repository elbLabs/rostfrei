import type { WorkloadMemoryState } from "@/lib/workload-memory"

export function WorkloadMemory({
  label,
  state,
}: {
  label?: string
  state?: WorkloadMemoryState
}) {
  if (!label || !state) return null
  const memory = state.status === "ready" ? state.byLabel.get(label) : undefined
  const limit = memory?.limit

  return (
    <div aria-label="Container memory">
      <strong>Container memory · {label}</strong>
      <div>
        {state.status === "loading"
          ? "Sampling…"
          : `Usage: ${formatBytes(memory?.currentBytes)} · Limit: ${limit?.status === "unlimited" ? "Unlimited" : formatBytes(limit?.status === "limited" ? limit.bytes : undefined)}`}
      </div>
      <small>
        Shared container sample, not memory attributed to this read model.
      </small>
    </div>
  )
}

function formatBytes(bytes: number | null | undefined) {
  if (bytes == null) return "Unavailable"
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"]
  const unit =
    bytes > 0
      ? Math.min(units.length - 1, Math.floor(Math.log2(bytes) / 10))
      : 0
  return `${(bytes / 1024 ** unit).toLocaleString(undefined, { maximumFractionDigits: 1 })} ${units[unit]}`
}
