import { lazy, Suspense } from 'react'
import { Network, X } from 'lucide-react'

import { GraphSidebarResizeHandle } from '@/components/graph-sidebar-resize-handle'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import type { DomainIndex, DomainKey } from '@/domain/index'
import { useSidebarLayoutStore } from '@/stores/sidebar-layout'

const DomainGraph = lazy(() => import('@/components/domain-graph').then((module) => ({
  default: module.DomainGraph,
})))

export function DomainGraphSidebar({
  index,
  selectedKey,
  onNavigate,
}: {
  index: DomainIndex
  selectedKey: DomainKey
  onNavigate: (key: DomainKey) => void
}) {
  const open = useSidebarLayoutStore((state) => state.graphOpen)
  const width = useSidebarLayoutStore((state) => state.graphWidth)
  const close = useSidebarLayoutStore((state) => state.setGraphOpen)

  if (!open) return null

  return (
    <aside
      aria-label="Domain graph sidebar"
      style={{ width }}
      className="relative hidden h-svh shrink-0 flex-col border-l border-white/10 bg-[#0d0f10] text-zinc-100 md:flex"
    >
      <GraphSidebarResizeHandle />
      <header aria-label="Domain graph header" className="flex h-14 shrink-0 items-center justify-between border-b border-white/10 px-3">
        <div className="flex min-w-0 items-center gap-2">
          <Network className="size-4 shrink-0 text-lime-300" />
          <div className="min-w-0">
            <p className="text-xs font-medium text-zinc-200">Domain graph</p>
            <p className="truncate text-[10px] text-zinc-600">Focused relationships</p>
          </div>
        </div>
        <Button variant="ghost" size="icon-sm" onClick={() => close(false)} aria-label="Close domain graph" title="Close domain graph">
          <X />
        </Button>
      </header>
      <div className="min-h-0 flex-1 p-3">
        <Suspense fallback={<div className="grid size-full place-items-center rounded-lg border border-white/10 bg-[#101314]"><Spinner className="text-zinc-500" /></div>}>
          <DomainGraph index={index} selectedKey={selectedKey} onNavigate={onNavigate} />
        </Suspense>
      </div>
    </aside>
  )
}
