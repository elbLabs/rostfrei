import { useEffect, useRef, useState } from "react"
import { Database, Download, RefreshCw } from "lucide-react"
import { Input } from "@rostfrei/ui/components/input"
import { Button } from "@/components/ui/button"
import {
  getHistoricalAggregate,
  getSnapshotFixture,
} from "@/lib/historical-api"
import type { HistoricalAggregate, HistoricalCapability } from "@/lib/types"

interface Props {
  capability: HistoricalCapability
  aggregateType: string
  aggregateId: string
  generation?: string
  timelineVersion?: string
}

export function HistoricalReconstruction({
  capability,
  aggregateType,
  aggregateId,
  generation,
  timelineVersion,
}: Props) {
  const [useSnapshots, setUseSnapshots] = useState(true)
  const [draft, setDraft] = useState("")
  const [version, setVersion] = useState("")
  const [revision, setRevision] = useState(0)
  const [loaded, setLoaded] = useState<{
    key: string
    value: HistoricalAggregate
  }>()
  const [error, setError] = useState<string>()
  const [selected, setSelected] = useState<string>()
  const [exporting, setExporting] = useState(false)
  const exportRequest = useRef<AbortController | undefined>(undefined)
  const [exportError, setExportError] = useState<string>()
  const [exported, setExported] = useState(false)
  const target = timelineVersion ?? version
  const readKey = JSON.stringify([
    capability.scope,
    aggregateType,
    aggregateId,
    generation,
    target,
    useSnapshots,
    revision,
  ])
  const result = loaded?.key === readKey ? loaded.value : undefined

  useEffect(() => {
    const controller = new AbortController()
    const params = new URLSearchParams({
      aggregateType,
      aggregateId,
      useSnapshots: String(useSnapshots),
    })
    if (target) params.set("version", target)
    if (generation) params.set("generation", generation)
    // Coalesce slider drags; a superseded read cannot paint state at a later version.
    const timer = window.setTimeout(
      () => {
        void getHistoricalAggregate(capability, params, controller.signal)
          .then((value) => {
            if (value && !controller.signal.aborted) {
              setLoaded({ key: readKey, value })
              setError(undefined)
            }
          })
          .catch((failure: unknown) => {
            if (!controller.signal.aborted) {
              setLoaded(undefined)
              setError(message(failure))
            }
          })
      },
      timelineVersion === undefined ? 0 : 120
    )
    return () => {
      window.clearTimeout(timer)
      controller.abort()
    }
  }, [
    capability,
    aggregateType,
    aggregateId,
    generation,
    target,
    timelineVersion,
    readKey,
    useSnapshots,
    revision,
  ])
  useEffect(() => () => exportRequest.current?.abort(), [])

  function reload() {
    exportRequest.current?.abort()
    setExporting(false)
    setLoaded(undefined)
    setError(undefined)
    setExportError(undefined)
    setExported(false)
    setRevision((value) => value + 1)
  }
  async function download() {
    const snapshot = result?.snapshots.find(
      (snapshot) => snapshot.streamVersion === selected
    )
    if (!result || !snapshot) return
    exportRequest.current?.abort()
    const controller = new AbortController()
    exportRequest.current = controller
    setExporting(true)
    setExportError(undefined)
    setExported(false)
    try {
      const fixtureId = `snapshot-${snapshot.streamVersion}`
      const params = new URLSearchParams({
        aggregateType,
        aggregateId,
        generation: result.generation,
        snapshotVersion: snapshot.streamVersion,
        version: result.version,
        fixtureId,
      })
      const fixture = await getSnapshotFixture(
        capability,
        params,
        controller.signal
      )
      if (controller.signal.aborted) return
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(fixture, null, 2) + "\n"], {
          type: "application/json",
        })
      )
      const link = document.createElement("a")
      link.href = url
      link.download = `${fixtureId}.json`
      link.click()
      window.setTimeout(() => URL.revokeObjectURL(url), 0)
      setExported(true)
    } catch (failure) {
      if (!controller.signal.aborted) setExportError(message(failure))
    } finally {
      if (!controller.signal.aborted) setExporting(false)
    }
  }
  const snapshot = result?.snapshots.find(
    (snapshot) => snapshot.streamVersion === selected
  )
  return (
    <details
      className={`historical-reconstruction ${timelineVersion === undefined ? "" : "timeline-reconstruction"}`}
      open
    >
      <summary>
        <Database size={14} /> Aggregate state &amp; snapshots
      </summary>
      <div className="reconstruction-body">
        <form
          className="reconstruction-controls"
          onSubmit={(event) => {
            event.preventDefault()
            setVersion(draft)
            reload()
          }}
        >
          <label className="snapshot-toggle">
            <input
              type="checkbox"
              aria-label="Use snapshots"
              checked={useSnapshots}
              onChange={(event) => {
                setUseSnapshots(event.target.checked)
                reload()
              }}
            />{" "}
            Use snapshots
          </label>
          <label>
            At version
            <Input
              aria-label="Historical target version"
              inputMode="numeric"
              pattern="[0-9]*"
              placeholder="Latest"
              value={timelineVersion ?? draft}
              readOnly={timelineVersion !== undefined}
              onChange={(event) => setDraft(event.target.value)}
            />
          </label>
          {timelineVersion === undefined ? (
            <Button
              variant="outline"
              size="sm"
              type="submit"
              aria-label="Reconstruct aggregate state"
            >
              <RefreshCw /> Reconstruct
            </Button>
          ) : (
            <span className="timeline-state-label">Follows timeline</span>
          )}
        </form>
        <p className="reconstruction-explanation">
          {useSnapshots
            ? "Restore a compatible snapshot, then replay later events."
            : "Replay original events from the beginning."}{" "}
          Original events remain in the graph.
        </p>
        <div className="reconstruction-result">
          {error ? (
            <p role="alert">
              {error} Refresh history to retry against its current generation.
            </p>
          ) : !result ? (
            <p role="status">Reconstructing aggregate state…</p>
          ) : (
            <>
              <p
                className="reconstruction-summary"
                data-reconstruction-status={result.snapshotStatus}
              >
                Version {result.version} ·{" "}
                {result.snapshotVersion === "0"
                  ? "Original-event replay"
                  : `Snapshot ${result.snapshotVersion} + tail`}{" "}
                · {result.eventsApplied} events applied · {result.eventsRead}{" "}
                read · {result.snapshotStatus}
              </p>
              <div className="reconstruction-columns">
                <section>
                  <h3>Reconstructed state</h3>
                  <State
                    value={result.state}
                    label="Reconstructed aggregate state"
                  />
                </section>
                <section className="snapshot-list">
                  <h3>Stored snapshots ({result.snapshots.length})</h3>
                  {result.snapshots.length === 0 && (
                    <p>
                      No snapshots stored for this history and replay revision.
                    </p>
                  )}
                  {result.snapshots.map((item) => (
                    <button
                      type="button"
                      key={item.streamVersion}
                      data-snapshot-version={item.streamVersion}
                      aria-label={`Inspect snapshot version ${item.streamVersion}`}
                      aria-pressed={item.streamVersion === selected}
                      onClick={() => {
                        setSelected(item.streamVersion)
                        setExportError(undefined)
                        setExported(false)
                      }}
                    >
                      <strong>Version {item.streamVersion}</strong>
                      <span>
                        {item.replayRevision} · schema {item.schemaVersion} ·{" "}
                        {item.stateBytes} bytes{item.valid ? "" : " · invalid"}
                      </span>
                    </button>
                  ))}
                  {snapshot && (
                    <div className="snapshot-details">
                      <State
                        value={snapshot.state}
                        label="Stored snapshot state"
                      />
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={
                          exporting ||
                          !snapshot.valid ||
                          !capability.snapshotFixtureHref ||
                          BigInt(snapshot.streamVersion) >
                            BigInt(result.version)
                        }
                        aria-label="Download snapshot fixture"
                        onClick={() => void download()}
                      >
                        <Download />{" "}
                        {exporting ? "Exporting…" : "Download fixture"}
                      </Button>
                      <p>
                        Includes the saved snapshot and every original event
                        through version {result.version}.
                      </p>
                      {exported && (
                        <p role="status">
                          Fixture downloaded. Register this JSON as a Test
                          fixture in the application.
                        </p>
                      )}
                    </div>
                  )}
                  {exportError && <p role="alert">{exportError}</p>}
                </section>
              </div>
            </>
          )}
        </div>
      </div>
    </details>
  )
}

function State({ value, label }: { value: unknown; label: string }) {
  return value === undefined ? (
    <p aria-label={label}>Redacted or unavailable.</p>
  ) : (
    <pre aria-label={label}>{JSON.stringify(value, null, 2)}</pre>
  )
}
function message(failure: unknown): string {
  return failure instanceof Error
    ? failure.message
    : "Snapshot inspection failed"
}
