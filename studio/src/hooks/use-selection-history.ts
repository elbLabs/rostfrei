import { useState } from 'react'

import type { DomainIndex, DomainKey } from '@/domain/index'

type SelectionHistory = {
  workspaceId?: string
  entries: DomainKey[]
  cursor: number
}

const emptyHistory: SelectionHistory = { entries: [], cursor: -1 }

export function useSelectionHistory(index: DomainIndex | undefined, workspaceId?: string) {
  const [history, setHistory] = useState<SelectionHistory>(emptyHistory)
  const current = index && workspaceId
    ? normalizeHistory(history, index, workspaceId)
    : emptyHistory
  const sameWorkspace = current.workspaceId === workspaceId
  const historyKey = sameWorkspace ? current.entries[current.cursor] : undefined
  const activeKey = historyKey && index?.selections.has(historyKey)
    ? historyKey
    : index?.initialSelection ?? null

  function navigate(key: DomainKey) {
    if (!index || !workspaceId || !index.selections.has(key)) return

    setHistory((current) => {
      const base = normalizeHistory(current, index, workspaceId)
      if (base.entries[base.cursor] === key) return base

      const entries = [...base.entries.slice(0, base.cursor + 1), key]
      return { workspaceId, entries, cursor: entries.length - 1 }
    })
  }

  function back() {
    if (!index || !workspaceId) return
    setHistory((current) => {
      const base = normalizeHistory(current, index, workspaceId)
      return base.cursor > 0 ? { ...base, cursor: base.cursor - 1 } : base
    })
  }

  function forward() {
    if (!index || !workspaceId) return
    setHistory((current) => {
      const base = normalizeHistory(current, index, workspaceId)
      return base.cursor < base.entries.length - 1 ? { ...base, cursor: base.cursor + 1 } : base
    })
  }

  return {
    activeKey,
    navigate,
    back,
    forward,
    canGoBack: sameWorkspace && current.cursor > 0,
    canGoForward: sameWorkspace && current.cursor < current.entries.length - 1,
  }
}

function initialHistory(index: DomainIndex, workspaceId: string): SelectionHistory {
  return index.initialSelection
    ? { workspaceId, entries: [index.initialSelection], cursor: 0 }
    : { workspaceId, entries: [], cursor: -1 }
}

function normalizeHistory(
  history: SelectionHistory,
  index: DomainIndex,
  workspaceId: string,
): SelectionHistory {
  if (history.workspaceId !== workspaceId) return initialHistory(index, workspaceId)

  const entries = history.entries.filter((key) => index.selections.has(key))
  if (!entries.length) return initialHistory(index, workspaceId)

  const cursor = history.entries
    .slice(0, history.cursor + 1)
    .filter((key) => index.selections.has(key)).length - 1

  return entries.length === history.entries.length && cursor === history.cursor
    ? history
    : { workspaceId, entries, cursor: Math.max(0, cursor) }
}
