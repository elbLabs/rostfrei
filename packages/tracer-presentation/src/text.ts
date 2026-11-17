import { assertPresentation, resolveSelection } from './evidence.ts'
import { EvidenceError } from './errors.ts'
import { jsonText, quote } from './json.ts'
import type { Json, JsonObject, Message, Presentation, RenderOptions } from './types.ts'

function rejectionCode(value: Json | undefined): string | undefined {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    const code = (value as JsonObject).code
    if (typeof code === 'string') return code
  }
}

function payload(lines: string[], value: { readonly payload?: Json }, enabled: boolean) {
  if (enabled && Object.hasOwn(value, 'payload')) lines.push(`  payload: ${jsonText(value.payload!)}`)
}

/** Deterministic plain text. Payloads and rejection details require explicit opt-in. */
export function renderText(view: Presentation, options: RenderOptions = {}): string {
  assertPresentation(view)
  const maximum = options.maxCharacters ?? 16000
  if (!Number.isSafeInteger(maximum) || maximum < 512 || maximum > 65536) throw new EvidenceError('OUTPUT_LIMIT', 'Text budget must be an integer from 512 to 65536 characters')
  const selected = options.selection ? resolveSelection(view, options.selection) : undefined
  const includePayloads = options.includePayloads === true
  const lines: string[] = []
  const limitations = () => {
    for (const diagnostic of view.diagnostics.slice(0, 20)) {
      lines.push(`Diagnostic ${diagnostic.code} at ${quote(diagnostic.path || '/')}: ${diagnostic.message}`)
      if (diagnostic.relatedPaths?.length) lines.push(`  related sources: ${diagnostic.relatedPaths.slice(0, 4).map((path) => quote(path)).join(', ')}`)
    }
    if (view.diagnostics.length > 20) lines.push(`Omitted ${view.diagnostics.length - 20} further diagnostics; retained in view.diagnostics.`)
    for (const warning of view.warnings.slice(0, 20)) lines.push(`Warning: ${warning}`)
    if (view.warnings.length > 20) lines.push(`Omitted ${view.warnings.length - 20} further warnings; retained in view.warnings.`)
  }
  if (view.kind === 'handlerSimulation') {
    const simulation = view.simulation
    lines.push(`Handler simulation: ${simulation.outcome.decision}; uncommitted.`)
    limitations()
    lines.push(`Command: ${quote(simulation.context)} / ${quote(simulation.command)} v${simulation.schemaVersion}`)
    const code = rejectionCode(simulation.outcome.rejection)
    if (code !== undefined) lines.push(`Rejection code: ${quote(code)}`)
    if (includePayloads && simulation.outcome.rejection !== undefined) lines.push(`Rejection: ${jsonText(simulation.outcome.rejection)}`)
    for (const stream of simulation.streams.slice(0, 40)) {
      lines.push(`Stream: ${quote(stream.aggregateType)} / ${quote(stream.aggregateId)}; base version ${stream.baseStreamVersion}; read guard ${stream.readGuard}`)
    }
    if (simulation.streams.length > 40) lines.push(`Omitted ${simulation.streams.length - 40} streams from this preview.`)
    lines.push(`Uncommitted proposals: ${simulation.proposedEvents.length}`)
    for (const event of simulation.proposedEvents.slice(0, 40)) {
      lines.push(`Proposal: ${quote(event.eventType)} v${event.schemaVersion}; ${quote(event.aggregateType)} / ${quote(event.aggregateId)}; proposed version ${event.proposedStreamVersion}`)
      payload(lines, event, includePayloads)
    }
    if (simulation.proposedEvents.length > 40) lines.push(`Omitted ${simulation.proposedEvents.length - 40} proposals from this preview.`)
  } else if (view.kind === 'incomplete') {
    lines.push(`Saved execution: ${view.runState}; operation unavailable.`)
    limitations()
  } else {
    const { operation, series } = view
    if (view.report) lines.push(`Behavioral test: ${view.report.status}`)
    lines.push(`Operation ${operation.status}; business decision: ${operation.result?.decision ?? 'not recorded'}.`)
    if (view.captureStatus === 'anomalous') lines.push('Capture contains anomalous records; recorded source decisions are not reconciled facts.')
    limitations()
    lines.push(`Operation ID: ${quote(operation.operationId)}; correlation ID: ${quote(operation.correlationId)}; mode: ${operation.mode}`)
    lines.push(`Command: ${quote(operation.context)} / ${quote(operation.command)} v${operation.schemaVersion}`)
    if (view.runState) lines.push(`Retrieval: runState=${view.runState}; captureState=${view.captureState ?? 'not recorded'}`)
    const code = rejectionCode(operation.result?.rejection)
    if (code !== undefined) lines.push(`Rejection code: ${quote(code)}`)
    if (includePayloads && operation.result?.rejection !== undefined) lines.push(`Rejection: ${jsonText(operation.result.rejection)}`)
    if (operation.failure && typeof operation.failure.code === 'string') lines.push(`Failure code: ${quote(operation.failure.code)}`)
    if (series) {
      lines.push(`Capture: ${series.capture.fidelity}; settled=${series.capture.settled}; settledFor=${quote(series.capture.settledFor)}`)
      if (series.capture.note !== undefined) lines.push(`Capture note: ${quote(series.capture.note)}`)
      lines.push(`Captured events: ${series.messageSeries.messages.filter((message) => message.kind !== 'command').length}`)
      const messages = selected ? [selected.message] : series.messageSeries.messages.slice(0, 40)
      for (const message of messages) renderMessage(lines, view, message, includePayloads)
      if (selected) lines.push(`Selected source: ${quote(selected.path)}; identity resolved against this immutable snapshot.`)
      else if (series.messageSeries.messages.length > 40) lines.push(`Omitted ${series.messageSeries.messages.length - 40} messages from this preview.`)
      const outcomes = selected ? (selected.outcome ? [selected.outcome] : []) : series.messageSeries.commandOutcomes.slice(0, 40)
      for (const outcome of outcomes) {
        const label = view.unassociatedOutcomeIds.includes(outcome.responseMessageId) ? 'Unassociated/anomalous outcome record' : 'Command outcome'
        lines.push(`${label}: ${outcome.outcome.status}; command=${quote(outcome.commandMessageId)}; response=${quote(outcome.responseMessageId)}`)
        const outcomeCode = rejectionCode(outcome.outcome.value)
        if (outcomeCode !== undefined) lines.push(`  rejection code: ${quote(outcomeCode)}`)
        if (includePayloads && outcome.outcome.status === 'rejected') lines.push(`  rejection: ${jsonText(outcome.outcome.value)}`)
      }
      if (!selected && series.messageSeries.commandOutcomes.length > 40) lines.push(`Omitted ${series.messageSeries.commandOutcomes.length - 40} outcomes from this preview.`)
    }
  }
  if (!includePayloads) lines.push('Payloads and rejection details hidden (opt in with includePayloads).')
  const text = lines.join('\n')
  if (text.length <= maximum) return text
  // The bounded sections above cap construction work. Reserve room for an exact
  // omitted-character count rather than silently clipping the final result.
  const kept = maximum - 96
  return `${text.slice(0, kept)}\n[Omitted ${text.length - kept} rendered characters; full evidence retained in sourceText.]`
}

function renderMessage(lines: string[], view: Extract<Presentation, { kind: 'operation' }>, message: Message, includePayloads: boolean) {
  const anomaly = view.anomalousMessageIds.includes(message.messageId) ? ' [anomalous record]' : ''
  lines.push(`Message ${quote(message.messageId)}: ${message.kind} ${quote(message.name)} v${message.schemaVersion}${anomaly}`)
  const edge = view.edges.find((item) => item.childMessageId === message.messageId)
  if (edge) {
    const parent = view.series!.messageSeries.messages.find((item) => item.messageId === edge.parentMessageId)!
    lines.push(`  caused by: ${quote(parent.name)}; causationId=${quote(message.causationId!)} equals parent.messageId=${quote(parent.messageId)}`)
  } else if (message.causationId) lines.push(`  recorded causationId: ${quote(message.causationId)} (causal association unavailable; see diagnostics)`)
  if (message.aggregate) lines.push(`  aggregate: ${quote(message.aggregate.type)} / ${quote(message.aggregate.id)}`)
  payload(lines, message, includePayloads)
}
