import { useEffect, useRef, type PointerEvent as ReactPointerEvent } from 'react'

import { useSidebarLayoutStore } from '@/stores/sidebar-layout'

const KEYBOARD_STEP = 12

export function GraphSidebarResizeHandle() {
  const width = useSidebarLayoutStore((state) => state.graphWidth)
  const setWidth = useSidebarLayoutStore((state) => state.setGraphWidth)
  const resetWidth = useSidebarLayoutStore((state) => state.resetGraphWidth)
  const drag = useRef<{ startX: number; startWidth: number } | null>(null)

  useEffect(() => {
    function stopDragging() {
      drag.current = null
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }

    function resize(event: PointerEvent) {
      if (!drag.current) return
      setWidth(drag.current.startWidth + drag.current.startX - event.clientX)
    }

    window.addEventListener('pointermove', resize)
    window.addEventListener('pointerup', stopDragging)
    window.addEventListener('pointercancel', stopDragging)
    return () => {
      window.removeEventListener('pointermove', resize)
      window.removeEventListener('pointerup', stopDragging)
      window.removeEventListener('pointercancel', stopDragging)
      stopDragging()
    }
  }, [setWidth])

  function startDragging(event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault()
    drag.current = { startX: event.clientX, startWidth: width }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }

  return (
    <div
      role="separator"
      aria-label="Resize domain graph"
      aria-orientation="vertical"
      aria-valuenow={width}
      tabIndex={0}
      onPointerDown={startDragging}
      onDoubleClick={resetWidth}
      onKeyDown={(event) => {
        if (event.key === 'ArrowLeft') setWidth(width + KEYBOARD_STEP)
        if (event.key === 'ArrowRight') setWidth(width - KEYBOARD_STEP)
      }}
      className="absolute inset-y-0 -left-1 z-30 w-2 cursor-col-resize touch-none outline-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-transparent after:transition-colors hover:after:bg-lime-300/60 focus-visible:after:bg-lime-300"
    />
  )
}
