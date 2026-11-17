import { assertPresentation } from './evidence.ts'
import { EvidenceError } from './errors.ts'
import { displayText, summarizeEvidence } from './summary.ts'
import type { MermaidOptions, Presentation } from './types.ts'

// Entity encoding is Mermaid syntax, not HTML interpolation. No input can become
// an identifier, directive, link, style, Markdown string or statement boundary.
function label(value: string): string {
  return value.replace(/[&<>"'`\\[\]{}|;#%]/gu, (char) => `#${char.codePointAt(0)};`)
}

/** A bounded observed graph. Outcome/verdict are a caption, never causal nodes. */
export function renderMermaid(view: Presentation, options: MermaidOptions = {}): string {
  assertPresentation(view)
  const maximum = options.maxNodes ?? 12
  if (!Number.isSafeInteger(maximum) || maximum < 1 || maximum > 80) throw new EvidenceError('OUTPUT_LIMIT', 'Mermaid node budget must be an integer from 1 to 80')
  if (view.kind !== 'operation') throw new EvidenceError('UNSUPPORTED_EVIDENCE', view.kind === 'handlerSimulation'
    ? 'Observed Mermaid flow is unsupported for uncommitted handler proposals'
    : 'Observed Mermaid flow is unavailable: no operation was recorded')
  if (!view.series) throw new EvidenceError('CAPTURE_UNAVAILABLE', `Finite capture is ${view.captureStatus}; no observed flow can be drawn`)
  const summary = summarizeEvidence(view)
  const messages = view.series.messageSeries.messages.slice(0, maximum)
  const aliases = new Map(messages.map((message, index) => [message.messageId, `n${index}`]))
  const edges = view.edges.filter((edge) => aliases.has(edge.parentMessageId) && aliases.has(edge.childMessageId))
  const omittedMessages = view.series.messageSeries.messages.length - messages.length
  const omittedEdges = view.edges.length - edges.length
  const omitted = `Omitted ${omittedMessages} message${omittedMessages === 1 ? '' : 's'} and ${omittedEdges} causal edge${omittedEdges === 1 ? '' : 's'}`
  const missingPayloads = view.series.messageSeries.messages.filter((message) => !Object.hasOwn(message, 'payload')).length
  const captionParts = [...summary.facts.filter((fact) => ['Command outcome', 'Test verdict', 'Capture'].includes(fact.label)).map((fact) => `${fact.label}: ${fact.value}`),
    'Downstream completion is not established.',
    ...(missingPayloads ? [`${missingPayloads} payload${missingPayloads === 1 ? '' : 's'} missing or redacted`] : []),
    ...summary.issues,
    ...(messages.length < view.series.messageSeries.messages.length ? [omitted] : []),
  ]
  const caption = captionParts.map((part) => part.endsWith('.') ? part : `${part}.`).join(' ')
  const lines = ['flowchart TD', '  accTitle: Observed message flow', `  accDescr: ${caption}`]
  for (const detail of [...summary.limitations, ...summary.diagnostics]) lines.push(`  %% ${label(detail)}`)
  if (!messages.length) {
    lines.push('  %% No messages in this finite capture. No absence or completion claim is inferred.')
    return lines.join('\n')
  }
  lines.push(`  subgraph evidence["${captionParts.map(label).join('<br/>')}"]`)
  for (const message of messages) {
    const anomaly = view.anomalousMessageIds.includes(message.messageId) ? ' (anomalous)' : ''
    lines.push(`  ${aliases.get(message.messageId)}["${label(`${message.kind}: ${displayText(message.name, 96)} v${message.schemaVersion}${anomaly}`)}"]`)
  }
  for (const edge of edges) lines.push(`  ${aliases.get(edge.parentMessageId)} --> ${aliases.get(edge.childMessageId)}`)
  lines.push('  end', '  style evidence fill:transparent,stroke:none')
  return lines.join('\n')
}
