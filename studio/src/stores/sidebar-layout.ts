import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export const DEFAULT_SIDEBAR_WIDTH = 300
export const MIN_SIDEBAR_WIDTH = 240
export const MAX_SIDEBAR_WIDTH = 440
export const DEFAULT_GRAPH_SIDEBAR_WIDTH = 380
export const MIN_GRAPH_SIDEBAR_WIDTH = 280
export const MAX_GRAPH_SIDEBAR_WIDTH = 640

type SidebarLayoutState = {
  width: number
  graphWidth: number
  graphOpen: boolean
  setWidth: (width: number) => void
  setGraphWidth: (width: number) => void
  setGraphOpen: (open: boolean) => void
  toggleGraph: () => void
  resetWidth: () => void
  resetGraphWidth: () => void
}

function clampWidth(width: number) {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, Math.round(width)))
}

function clampGraphWidth(width: number) {
  return Math.min(MAX_GRAPH_SIDEBAR_WIDTH, Math.max(MIN_GRAPH_SIDEBAR_WIDTH, Math.round(width)))
}

export const useSidebarLayoutStore = create<SidebarLayoutState>()(
  persist(
    (set) => ({
      width: DEFAULT_SIDEBAR_WIDTH,
      graphWidth: DEFAULT_GRAPH_SIDEBAR_WIDTH,
      graphOpen: true,
      setWidth: (width) => set({ width: clampWidth(width) }),
      setGraphWidth: (graphWidth) => set({ graphWidth: clampGraphWidth(graphWidth) }),
      setGraphOpen: (graphOpen) => set({ graphOpen }),
      toggleGraph: () => set((state) => ({ graphOpen: !state.graphOpen })),
      resetWidth: () => set({ width: DEFAULT_SIDEBAR_WIDTH }),
      resetGraphWidth: () => set({ graphWidth: DEFAULT_GRAPH_SIDEBAR_WIDTH }),
    }),
    {
      name: 'rostfrei-studio:sidebar-layout',
      partialize: ({ width, graphWidth, graphOpen }) => ({ width, graphWidth, graphOpen }),
    },
  ),
)
