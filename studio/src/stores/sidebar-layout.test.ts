import { beforeEach, describe, expect, it } from 'vitest'

import {
  DEFAULT_SIDEBAR_WIDTH,
  DEFAULT_GRAPH_SIDEBAR_WIDTH,
  MAX_GRAPH_SIDEBAR_WIDTH,
  MAX_SIDEBAR_WIDTH,
  MIN_GRAPH_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
  useSidebarLayoutStore,
} from './sidebar-layout'

describe('sidebar layout store', () => {
  beforeEach(() => {
    localStorage.clear()
    useSidebarLayoutStore.getState().resetWidth()
    useSidebarLayoutStore.getState().resetGraphWidth()
    useSidebarLayoutStore.getState().setGraphOpen(true)
  })

  it('clamps and persists the sidebar width', () => {
    useSidebarLayoutStore.getState().setWidth(MAX_SIDEBAR_WIDTH + 100)
    expect(useSidebarLayoutStore.getState().width).toBe(MAX_SIDEBAR_WIDTH)

    useSidebarLayoutStore.getState().setWidth(MIN_SIDEBAR_WIDTH - 100)
    expect(useSidebarLayoutStore.getState().width).toBe(MIN_SIDEBAR_WIDTH)

    const saved = JSON.parse(localStorage.getItem('rostfrei-studio:sidebar-layout') ?? '{}')
    expect(saved.state.width).toBe(MIN_SIDEBAR_WIDTH)

    useSidebarLayoutStore.getState().resetWidth()
    expect(useSidebarLayoutStore.getState().width).toBe(DEFAULT_SIDEBAR_WIDTH)
  })

  it('clamps and persists the graph sidebar layout', () => {
    useSidebarLayoutStore.getState().setGraphWidth(MAX_GRAPH_SIDEBAR_WIDTH + 100)
    expect(useSidebarLayoutStore.getState().graphWidth).toBe(MAX_GRAPH_SIDEBAR_WIDTH)

    useSidebarLayoutStore.getState().setGraphWidth(MIN_GRAPH_SIDEBAR_WIDTH - 100)
    useSidebarLayoutStore.getState().setGraphOpen(false)

    expect(useSidebarLayoutStore.getState().graphWidth).toBe(MIN_GRAPH_SIDEBAR_WIDTH)
    expect(useSidebarLayoutStore.getState().graphOpen).toBe(false)

    const saved = JSON.parse(localStorage.getItem('rostfrei-studio:sidebar-layout') ?? '{}')
    expect(saved.state).toMatchObject({
      graphWidth: MIN_GRAPH_SIDEBAR_WIDTH,
      graphOpen: false,
    })

    useSidebarLayoutStore.getState().resetGraphWidth()
    expect(useSidebarLayoutStore.getState().graphWidth).toBe(DEFAULT_GRAPH_SIDEBAR_WIDTH)
  })
})
