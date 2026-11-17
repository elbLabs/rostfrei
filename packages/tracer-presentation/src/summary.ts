import { assertPresentation } from './evidence.ts'
import { freeze } from './json.ts'
import type { EvidenceSummary, Presentation } from './types.ts'

const issueMessages: Readonly<Record<string, string>> = {
  PROVENANCE_UNKNOWN: 'Release information is incomplete.',
  PROVENANCE_DISAGREEMENT: 'Recorded releases disagree.',
  SOURCE_LEXEMES_UNAVAILABLE: 'Original file bytes are unavailable.',
  INVALID_CAPTURE: 'Capture unavailable: validation failed.',
  MISSING_PARENT: 'A parent message is missing from the capture.',
  CROSS_CORRELATION: 'Recorded correlation identifiers disagree.',
  CAUSATION_CYCLE: 'Recorded causal links form a cycle.',
  INVALID_REPORT_SECTION: 'Part of the report could not be validated.',
  MISSING_COMMAND: 'An outcome has no matching captured command.',
  OUTCOME_CONFLICT: 'Recorded command outcomes conflict.',
  OPERATION_STATE_CONFLICT: 'The command outcome conflicts with the operation state.',
}

/** Plain display text, not HTML/Markdown. Controls cannot impersonate another line. */
export function displayText(value: string, maximum = 240): string {
  const safe = value.replace(/[\u0000-\u001f\u007f-\u009f\u2028-\u202e\u2066-\u2069]/g,
    (char) => `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`)
  return safe.length <= maximum ? safe : `${safe.slice(0, maximum)}… [label clipped]`
}

/** Wire-safe, payload-free first screen. Interpretation stays in the reader's host. */
export function summarizeEvidence(view: Presentation): EvidenceSummary {
  assertPresentation(view)
  const facts: EvidenceSummary['facts'][number][] = []
  const fact = (label: string, value: string, tone: EvidenceSummary['facts'][number]['tone'] = 'neutral') => facts.push({ label, value: displayText(value), tone })
  let title: string
  if (view.kind === 'operation') {
    title = displayText(view.operation.command, 96)
    const conflict = view.diagnostics.some((item) => ['OUTCOME_CONFLICT', 'OPERATION_STATE_CONFLICT'].includes(item.code))
    fact('Command outcome', conflict ? 'conflicting records' : view.operation.result?.decision ?? 'not recorded', conflict ? 'warning' : 'neutral')
    fact('Test verdict', typeof view.report?.status === 'string' ? view.report.status : 'not recorded', view.report?.status === 'passed' ? 'positive' : view.report?.status === 'failed' ? 'negative' : 'neutral')
    const rejection = view.operation.result?.rejection
    if (!conflict && rejection && typeof rejection === 'object' && 'code' in rejection && typeof rejection.code === 'string') fact('Rejection code', rejection.code)
    fact('Operation', `${view.operation.status} in ${view.operation.mode} mode`)
    const capture = view.series?.capture
    const captureNotes = [capture?.fidelity === 'grouped' ? 'grouped evidence' : '', view.captureStatus === 'anomalous' ? 'anomalous records' : ''].filter(Boolean)
    fact('Capture', capture ? `${capture.settled ? 'settled' : 'partial'}${captureNotes.length ? ` with ${captureNotes.join(' and ')}` : ''}` : view.captureStatus,
      !capture || !capture.settled || capture.fidelity === 'grouped' || view.captureStatus === 'anomalous' ? 'warning' : 'neutral')
  } else if (view.kind === 'handlerSimulation') {
    title = 'Handler simulation — uncommitted'
    fact('Handler decision', view.simulation.outcome.decision)
    fact('Test verdict', 'not available')
    fact('Observed flow', 'unsupported — proposals are not observed messages', 'warning')
  } else {
    title = 'Incomplete saved run'
    fact('Submission', view.runState, 'warning')
    fact('Command outcome', 'not recorded')
    fact('Test verdict', 'not recorded')
    fact('Observed flow', 'unavailable', 'warning')
  }
  const limitations = view.warnings.slice(0, 6).map((warning) => warning === 'Finite capture only; retrieval and settling do not prove downstream business completion.'
    ? 'Downstream completion is not established.' : displayText(warning, 300))
  if (view.warnings.length > 6) limitations.push(`Omitted ${view.warnings.length - 6} further limitations; inspect the saved evidence.`)
  const diagnostics = view.diagnostics.slice(0, 6).map((item) => `${item.code}: ${displayText(item.message, 200)} (${displayText(item.path || '/', 120)})`)
  if (view.diagnostics.length > 6) diagnostics.push(`Omitted ${view.diagnostics.length - 6} further diagnostics; inspect the saved evidence.`)
  const reasons = [...new Set(view.diagnostics.map((item) => Object.hasOwn(issueMessages, item.code) ? issueMessages[item.code] : displayText(item.message, 160)))]
  const issues = reasons.slice(0, 6)
  if (reasons.length > 6) issues.push(`${reasons.length - 6} further issues in evidence details.`)
  return freeze({ title, facts, limitations, issues, diagnostics })
}
