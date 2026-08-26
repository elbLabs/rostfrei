import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

import { buildDomainIndex, type DomainIndex } from '@/domain/index'
import { parseDomainModel } from '@/domain/schema'

export type Diagnostic = {
  level: string
  message: string
  rendered?: string
  file?: string
  line?: number
}

export type CheckResult = {
  success: boolean
  diagnostics: Diagnostic[]
}

export async function openWorkspace(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false })
  return typeof selected === 'string' ? selected : null
}

export async function loadDomainModel(
  workspacePath: string,
  packageName?: string,
): Promise<DomainIndex> {
  const model = await invoke<unknown>('load_domain_model', {
    workspacePath,
    package: packageName,
  })
  return buildDomainIndex(parseDomainModel(model))
}

export function checkWorkspace(workspacePath: string): Promise<CheckResult> {
  return invoke<CheckResult>('check_workspace', { workspacePath })
}
