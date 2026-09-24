import { LoaderCircle, RefreshCw, Unplug } from "lucide-react"

import { Button } from "@/components/ui/button"

export function ConnectionNotice({
  connecting,
  testsOnly = false,
  error,
  onRetry,
}: {
  connecting: boolean
  testsOnly?: boolean
  error?: string
  onRetry: () => void
}) {
  return (
    <section
      className="connection-notice"
      data-connection={
        connecting
          ? "connecting"
          : testsOnly
            ? "tests-unavailable"
            : "disconnected"
      }
      role={connecting ? "status" : "alert"}
    >
      <div className="connection-notice-heading">
        {connecting ? (
          <LoaderCircle size={20} className="animate-spin" />
        ) : (
          <Unplug size={20} />
        )}
        <div>
          <h1>
            {connecting
              ? "Connecting to Tracer…"
              : testsOnly
                ? "Test discovery failed"
                : "Tracer connection failed"}
          </h1>
          <p>
            {connecting
              ? "Loading tests and fixture state from the Tracer API."
              : testsOnly
                ? "Tracer is connected. Commands are available, but behavioral tests could not be loaded."
                : "Live execution is unavailable. Connect to Tracer to load tests and run them in isolated Test state."}
          </p>
        </div>
        {!connecting && (
          <Button className="connection-retry" onClick={onRetry}>
            <RefreshCw /> Retry connection
          </Button>
        )}
      </div>
      {!connecting && (
        <>
          <p className="connection-error-detail">
            {error ?? "The Tracer API could not be loaded."}
          </p>
          <details className="connection-help">
            <summary>Connection settings</summary>
            <p>
              Start Tracer and set <code>VITE_TRACER_TARGET</code> to its
              reachable HTTP address (default:{" "}
              <code>http://127.0.0.1:1309</code>). Restart the Studio server
              after changing the proxy target.
            </p>
            <p>
              In Docker, <code>127.0.0.1</code> refers to the Studio container.
              Use the Tracer container’s network address or an explicitly
              configured host connection.
            </p>
            <p>
              For authentication, set <code>VITE_TRACER_TOKEN</code> to the
              Tracer control token and rebuild the frontend.
            </p>
          </details>
        </>
      )}
    </section>
  )
}
