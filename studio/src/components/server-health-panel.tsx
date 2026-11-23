import { RefreshCw } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  failureLabel,
  serverHealthLabel,
  type ServerSample,
  type useServerHealth,
} from "./server-health-data"
import "./server-health-panel.css"

export function ServerHealthPanel({
  sample,
  label,
}: {
  sample: NonNullable<ReturnType<typeof useServerHealth>>
  label?: string
}) {
  const { result, refresh, loading, reading } = sample
  return (
    <section
      className="server-health-panel"
      aria-label="Server monitoring"
      aria-busy={loading}
    >
      <header className="server-health-toolbar">
        <div>
          <p>
            Health and process memory for configured nodes, separate from
            JetStream account quotas.
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={loading}
          onClick={refresh}
        >
          <RefreshCw aria-hidden="true" /> Refresh server health
        </Button>
      </header>
      {loading ? (
        <p role="status">
          Sampling server nodes… {reading && "Showing the previous sample."}
        </p>
      ) : (
        result?.status === "error" && (
          <p role="alert">
            Could not load server monitoring. {result.message}{" "}
            {reading && "Showing the previous sample."}
          </p>
        )
      )}
      {reading && (
        <>
          <p className="server-health-time">
            Received by Studio{" "}
            <time dateTime={reading.received.toISOString()}>
              {reading.received.toLocaleString()}
            </time>
          </p>
          <ul className="server-health-nodes">
            {reading.servers
              .filter((server) => label === undefined || server.label === label)
              .map((server) => (
                <ServerNode key={server.label} server={server} />
              ))}
            {!reading.servers.length && <li>No server nodes reported.</li>}
          </ul>
        </>
      )}
    </section>
  )
}

function ServerNode({ server }: { server: ServerSample }) {
  const { health, metrics } = server
  const healthy = health.status === "available" && health.value === "healthy"
  return (
    <li className="server-health-node">
      <div className="server-health-node-heading">
        <h3>{server.label}</h3>
        <span className="server-health-status" data-warning={!healthy}>
          {serverHealthLabel(server)}
        </span>
      </div>
      {metrics.status === "unavailable" ? (
        <p>Process metrics: {failureLabel[metrics.reason]}.</p>
      ) : (
        <dl className="server-health-facts">
          <div>
            <dt>Process RAM (RSS)</dt>
            <dd>
              {metrics.value.residentMemoryBytes === undefined
                ? "Not reported"
                : bytes(metrics.value.residentMemoryBytes)}
            </dd>
          </div>
          {metrics.value.version !== undefined && (
            <div>
              <dt>Version</dt>
              <dd>{metrics.value.version}</dd>
            </div>
          )}
          {metrics.value.uptime !== undefined && (
            <div>
              <dt>Uptime</dt>
              <dd>{metrics.value.uptime}</dd>
            </div>
          )}
          {metrics.value.jetStreamLimits?.maxMemoryBytes !== undefined && (
            <div>
              <dt>JetStream server memory limit</dt>
              <dd>{bytes(metrics.value.jetStreamLimits.maxMemoryBytes)}</dd>
            </div>
          )}
          {metrics.value.jetStreamLimits?.maxStorageBytes !== undefined && (
            <div>
              <dt>JetStream server storage limit</dt>
              <dd>{bytes(metrics.value.jetStreamLimits.maxStorageBytes)}</dd>
            </div>
          )}
        </dl>
      )}
    </li>
  )
}

const numberFormat = new Intl.NumberFormat(undefined, {
  maximumFractionDigits: 2,
})

function bytes(value: number): string {
  for (const [power, unit] of [
    [6, "EiB"],
    [5, "PiB"],
    [4, "TiB"],
    [3, "GiB"],
    [2, "MiB"],
    [1, "KiB"],
  ] as const) {
    const scale = 1024 ** power
    if (value >= scale) return `${numberFormat.format(value / scale)} ${unit}`
  }
  return `${numberFormat.format(value)} B`
}
