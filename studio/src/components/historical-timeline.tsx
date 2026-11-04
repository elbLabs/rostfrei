import { useEffect, useState } from "react"
import { Pause, Play, SkipBack, SkipForward } from "lucide-react"
import { Button } from "@/components/ui/button"
import { milestoneTime, MAX_TIMELINE_RECORDS } from "@/lib/historical-timeline"
import type { HistoricalMessage } from "@/lib/types"

interface Props {
  items: HistoricalMessage[]
  position: number
  onSeek: (position: number) => void
  more: boolean
  busy: boolean
  onLoadMore: () => void
}

export function HistoricalTimeline({
  items,
  position,
  onSeek,
  more,
  busy,
  onLoadMore,
}: Props) {
  const [playing, setPlaying] = useState(false)
  const current = items[position - 1]
  const running = playing && position < items.length
  useEffect(() => {
    if (!running) return
    const timer = window.setTimeout(() => {
      onSeek(position + 1)
      if (position + 1 === items.length) setPlaying(false)
    }, 1000)
    return () => window.clearTimeout(timer)
  }, [running, position, items.length, onSeek])

  function seek(value: number) {
    setPlaying(false)
    onSeek(value)
  }
  return (
    <section className="historical-timeline" aria-label="Historical playback">
      <div className="timeline-heading">
        <strong>Timeline</strong>
        <span>
          {position} / {items.length} milestones
        </span>
        <span className="timeline-rate">
          1 milestone / s · idle gaps compressed
        </span>
        {more && (
          <Button
            variant="outline"
            size="sm"
            disabled={busy || items.length >= MAX_TIMELINE_RECORDS}
            aria-label="Load more historical milestones"
            onClick={onLoadMore}
          >
            {busy ? "Loading…" : "Load more"}
          </Button>
        )}
      </div>
      <div className="timeline-controls">
        <Button
          variant="ghost"
          size="icon"
          aria-label="Rewind historical timeline"
          disabled={!items.length}
          onClick={() => seek(0)}
        >
          <SkipBack />
        </Button>
        <Button
          variant="outline"
          size="icon"
          aria-label={
            running ? "Pause historical timeline" : "Play historical timeline"
          }
          disabled={!items.length}
          onClick={() => {
            if (running) setPlaying(false)
            else {
              if (position === items.length) onSeek(0)
              setPlaying(true)
            }
          }}
        >
          {running ? <Pause /> : <Play />}
        </Button>
        <input
          type="range"
          aria-label="Historical timeline"
          aria-valuetext={`${position} of ${items.length} milestones, ${milestoneTime(current)}`}
          min={0}
          max={items.length}
          step={1}
          value={position}
          disabled={!items.length}
          onChange={(event) => seek(Number(event.target.value))}
        />
        <Button
          variant="ghost"
          size="icon"
          aria-label="Go to loaded timeline end"
          disabled={!items.length}
          onClick={() => seek(items.length)}
        >
          <SkipForward />
        </Button>
      </div>
      <div className="timeline-current">
        <time title={current?.storedAt}>{milestoneTime(current)}</time>
        <span>
          {current
            ? `${current.name} · ${current.response?.status ?? (current.kind === "command" ? "request stored" : "persisted")}`
            : "Drag or play to reveal recorded messages."}
        </span>
      </div>
      <div className="timeline-range">
        <span>
          Loaded:{" "}
          {items.length
            ? `${milestoneTime(items[0])} → ${milestoneTime(items.at(-1))}`
            : "No matching milestones in this scan"}
        </span>
        <span>
          {items.length >= MAX_TIMELINE_RECORDS && more
            ? "1,000-milestone limit · narrow your selection"
            : more
              ? "More history available"
              : "End of this retained range"}
        </span>
      </div>
    </section>
  )
}
