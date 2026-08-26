import { useReducer } from 'react'

import type { DomainIndex } from '@/domain/index'
import {
  checkWorkspace,
  loadDomainModel,
  openWorkspace,
  type Diagnostic,
} from '@/lib/compiler'

// TODO: Select the model package from workspace metadata once multi-package workspaces are supported.
const MODEL_PACKAGE = 'bike-rental'

type WorkspaceData = {
  workspacePath: string
  workspaceName: string
  index: DomainIndex
}

export type WorkspaceState =
  | { status: 'noWorkspace' }
  | { status: 'loading'; workspacePath: string; workspaceName: string }
  | ({ status: 'valid'; diagnostics: Diagnostic[] } & WorkspaceData)
  | ({ status: 'checking'; diagnostics: Diagnostic[] } & WorkspaceData)
  | ({ status: 'invalid'; diagnostics: Diagnostic[] } & WorkspaceData)
  | {
      status: 'error'
      workspacePath?: string
      workspaceName?: string
      index?: DomainIndex
      diagnostics: Diagnostic[]
      message: string
      retry: 'load' | 'check'
    }

type Action =
  | { type: 'loading'; workspacePath: string; workspaceName: string }
  | ({ type: 'valid'; diagnostics?: Diagnostic[] } & WorkspaceData)
  | ({ type: 'checking' } & WorkspaceData & { diagnostics: Diagnostic[] })
  | ({ type: 'invalid' } & WorkspaceData & { diagnostics: Diagnostic[] })
  | {
      type: 'error'
      workspacePath?: string
      workspaceName?: string
      index?: DomainIndex
      diagnostics?: Diagnostic[]
      message: string
      retry: 'load' | 'check'
    }

function reducer(_: WorkspaceState, action: Action): WorkspaceState {
  switch (action.type) {
    case 'loading': return { status: 'loading', workspacePath: action.workspacePath, workspaceName: action.workspaceName }
    case 'valid': return { status: 'valid', workspacePath: action.workspacePath, workspaceName: action.workspaceName, index: action.index, diagnostics: action.diagnostics ?? [] }
    case 'checking': return { status: 'checking', workspacePath: action.workspacePath, workspaceName: action.workspaceName, index: action.index, diagnostics: action.diagnostics }
    case 'invalid': return { status: 'invalid', workspacePath: action.workspacePath, workspaceName: action.workspaceName, index: action.index, diagnostics: action.diagnostics }
    case 'error': return { status: 'error', workspacePath: action.workspacePath, workspaceName: action.workspaceName, index: action.index, diagnostics: action.diagnostics ?? [], message: action.message, retry: action.retry }
  }
}

function folderName(path: string): string {
  const normalized = path.replace(/[\\/]+$/, '')
  return normalized.split(/[\\/]/).pop() || normalized
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function useWorkspace() {
  const [state, dispatch] = useReducer(reducer, { status: 'noWorkspace' })

  async function load(workspacePath: string, workspaceName = folderName(workspacePath)) {
    dispatch({ type: 'loading', workspacePath, workspaceName })
    try {
      const index = await loadDomainModel(workspacePath, MODEL_PACKAGE)
      dispatch({ type: 'valid', workspacePath, workspaceName, index })
    } catch (error) {
      dispatch({ type: 'error', workspacePath, workspaceName, message: errorMessage(error), retry: 'load' })
    }
  }

  async function chooseWorkspace() {
    try {
      const workspacePath = await openWorkspace()
      if (workspacePath) await load(workspacePath)
    } catch (error) {
      dispatch({ type: 'error', message: errorMessage(error), retry: 'load' })
    }
  }

  async function check() {
    if (!('index' in state) || !state.index || !state.workspacePath || !state.workspaceName) return
    const current = {
      workspacePath: state.workspacePath,
      workspaceName: state.workspaceName,
      index: state.index,
      diagnostics: state.diagnostics,
    }
    dispatch({ type: 'checking', ...current })
    try {
      const result = await checkWorkspace(current.workspacePath)
      if (!result.success) {
        dispatch({ type: 'invalid', ...current, diagnostics: result.diagnostics })
        return
      }
      try {
        const index = await loadDomainModel(current.workspacePath, MODEL_PACKAGE)
        dispatch({ type: 'valid', ...current, index, diagnostics: result.diagnostics })
        return index
      } catch (error) {
        dispatch({ type: 'error', ...current, message: errorMessage(error), retry: 'check' })
      }
    } catch (error) {
      dispatch({ type: 'error', ...current, message: errorMessage(error), retry: 'check' })
    }
  }

  function retry() {
    if (state.status !== 'error') return
    if (state.retry === 'check' && state.index) void check()
    else if (state.workspacePath) void load(state.workspacePath, state.workspaceName)
    else void chooseWorkspace()
  }

  return { state, chooseWorkspace, check, retry }
}
