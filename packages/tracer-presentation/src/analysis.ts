import { comparableJson, object } from './json.ts'
import { checkObserved, checkOutcome } from './schema.ts'
import type { CommandOutcome, Diagnostic, Json, JsonObject, Operation, OperationSeries } from './types.ts'

export function redactedRejection(value: unknown): boolean {
  return !!value && typeof value === 'object' && !Array.isArray(value)
    && (('redacted' in value && value.redacted === true) || ('code' in value && value.code === 'REDACTED'))
}

interface Fact {
  path: string
  commandMessageId?: string
  responseMessageId?: string
  correlationId?: string
  decision: string
  rejection?: Json
}

/** Recorded identity matches are candidates, not necessarily valid causal facts. */
export function analyseSeries(series: OperationSeries | undefined, operation: Operation, report: JsonObject | undefined, diagnostics: Diagnostic[], base: string) {
  const messages = new Map((series?.messageSeries.messages ?? []).map((message, index) => [message.messageId, { message, path: `${base}/messageSeries/messages/${index}` }]))
  const capturedOutcomes = series?.messageSeries.commandOutcomes ?? []
  const anomalous = new Set<string>()
  const unassociated = new Set<string>()
  const add = (code: string, path: string, message: string, messageIds: string[], relatedPaths: string[] = [], responseMessageIds: string[] = []) => {
    diagnostics.push({ code, path, message, messageIds, relatedPaths, responseMessageIds, severity: 'error' })
  }
  for (const { message, path } of messages.values()) {
    if (message.correlationId !== operation.correlationId) {
      anomalous.add(message.messageId)
      add('CROSS_CORRELATION', `${path}/correlationId`, 'Message correlation disagrees with the capture envelope; causal associations withheld.', [message.messageId])
    }
    if (message.causationId) {
      const parent = messages.get(message.causationId)
      if (!parent) diagnostics.push({ code: 'MISSING_PARENT', severity: 'warning', path: `${path}/causationId`, message: 'Missing parent in this finite capture; left disconnected.', messageIds: [message.messageId] })
      else if (parent.message.correlationId !== message.correlationId) {
        anomalous.add(message.messageId)
        add('CROSS_CORRELATION', `${path}/causationId`, 'Parent and child correlations disagree; association withheld.', [message.messageId], [parent.path])
      }
    }
  }

  // A functional parent graph permits linear iterative cycle detection, including
  // self-links. No recursive traversal or guessed cycle-breaking edge.
  const visited = new Set<string>()
  for (const start of messages.keys()) {
    const path: string[] = []
    const positions = new Map<string, number>()
    let id: string | undefined = start
    while (id && messages.has(id) && !visited.has(id)) {
      const position = positions.get(id)
      if (position !== undefined) {
        const cycle = path.slice(position)
        for (const member of cycle) anomalous.add(member)
        add('CAUSATION_CYCLE', messages.get(id)!.path, 'Cyclic causation IDs are recorded; affected associations are not ordinary causal edges.', cycle, cycle.map((member) => messages.get(member)!.path))
        break
      }
      positions.set(id, path.length)
      path.push(id)
      id = messages.get(id)!.message.causationId ?? undefined
    }
    for (const member of path) visited.add(member)
  }

  const facts: Fact[] = []
  const result = operation.result
  const operationPath = report ? '/report/operation/result' : '/result'
  if (result) facts.push({ ...result, path: operationPath, correlationId: operation.correlationId })
  const outcomeFact = (outcome: CommandOutcome, path: string): Fact => ({
    commandMessageId: outcome.commandMessageId, responseMessageId: outcome.responseMessageId,
    correlationId: outcome.correlationId, decision: outcome.outcome.status,
    rejection: outcome.outcome.status === 'rejected' ? outcome.outcome.value : undefined, path,
  })
  const reportFacts: Fact[] = []
  if (report) {
    for (const field of ['commandOutcome', 'observed'] as const) {
      if (report[field] === undefined) continue
      try {
        if (field === 'commandOutcome') {
          checkOutcome(report[field], '/report/commandOutcome')
          reportFacts.push(outcomeFact(report[field] as unknown as CommandOutcome, '/report/commandOutcome'))
        } else {
          checkObserved(report[field], '/report/observed')
          const observed = object(report[field], '/report/observed')
          for (const [index, outcome] of (observed.commandOutcomes as unknown as CommandOutcome[]).entries()) {
            reportFacts.push(outcomeFact(outcome, `/report/observed/commandOutcomes/${index}`))
          }
        }
      } catch {
        diagnostics.push({ code: 'INVALID_REPORT_SECTION', severity: 'error', path: `/report/${field}`, message: 'Report section violates the canonical observed contract; retained in source but not used for association.' })
      }
    }
  }
  facts.push(...reportFacts)
  for (const [index, outcome] of capturedOutcomes.entries()) {
    const path = `${base}/messageSeries/commandOutcomes/${index}`
    const command = messages.get(outcome.commandMessageId)
    if (!command || command.message.kind !== 'command') {
      unassociated.add(outcome.responseMessageId)
      add('MISSING_COMMAND', path, 'Outcome has a missing command or references a non-command; left unassociated.', [outcome.commandMessageId], [], [outcome.responseMessageId])
    } else if (outcome.correlationId !== operation.correlationId || outcome.correlationId !== command.message.correlationId) {
      unassociated.add(outcome.responseMessageId)
      add('CROSS_CORRELATION', `${path}/correlationId`, 'Outcome correlation disagrees with its command or capture; left unassociated.', [outcome.commandMessageId], [command.path], [outcome.responseMessageId])
    }
    facts.push(outcomeFact(outcome, path))
  }

  const conflictingCommands = new Set<string>()
  function compare(left: Fact, right: Fact) {
    const differences: string[] = []
    for (const field of ['commandMessageId', 'responseMessageId', 'correlationId', 'decision'] as const) {
      if (left[field] !== undefined && right[field] !== undefined && left[field] !== right[field]) differences.push(field)
    }
    if (left.decision === 'rejected' && right.decision === 'rejected' && !redactedRejection(left.rejection) && !redactedRejection(right.rejection)) {
      const l = left.rejection
      const r = right.rejection
      if (l && r && typeof l === 'object' && typeof r === 'object' && !Array.isArray(l) && !Array.isArray(r)) {
        for (const field of ['code', 'classification', 'message', 'details', 'payload']) {
          const a = (l as JsonObject)[field]
          const b = (r as JsonObject)[field]
          if (a !== undefined && b !== undefined && comparableJson(a) !== comparableJson(b)) differences.push(`rejection.${field}`)
        }
      }
    }
    if (!differences.length) return
    const ids = [...new Set([left.commandMessageId, right.commandMessageId].filter((id): id is string => id !== undefined))]
    ids.forEach((id) => conflictingCommands.add(id))
    add('OUTCOME_CONFLICT', left.path, `Recorded sources disagree on ${differences.join(', ')}; no outcome is chosen as authoritative.`, ids, [right.path])
  }
  // The report's commandOutcome explicitly describes the root. Other observed
  // outcomes are compared by command identity, never by arrival or array position.
  const rootReport = reportFacts.find((fact) => fact.path === '/report/commandOutcome')
  if (result && rootReport) compare(facts[0], rootReport)
  const byCommand = new Map<string, Fact[]>()
  const byResponse = new Map<string, Fact[]>()
  for (const fact of facts) {
    if (fact.commandMessageId) {
      const group = byCommand.get(fact.commandMessageId) ?? []
      // A valid section has one outcome per command; bound malformed report
      // redundancy before comparing, even though source is already size-bounded.
      if (group.length < 4) {
        for (const earlier of group) compare(earlier, fact)
        group.push(fact)
        byCommand.set(fact.commandMessageId, group)
      }
    }
    if (fact.responseMessageId) {
      const group = byResponse.get(fact.responseMessageId) ?? []
      if (group.length < 4) {
        for (const earlier of group) if (earlier.commandMessageId !== fact.commandMessageId) compare(earlier, fact)
        group.push(fact)
        byResponse.set(fact.responseMessageId, group)
      }
    }
  }
  for (const outcome of capturedOutcomes) {
    if (conflictingCommands.has(outcome.commandMessageId)) unassociated.add(outcome.responseMessageId)
  }
  for (const id of conflictingCommands) if (messages.has(id)) anomalous.add(id)
  if (result && operation.status !== 'completed') {
    add('OPERATION_STATE_CONFLICT', operationPath, 'A business result and a non-completed operation state disagree; both records are retained.', result.commandMessageId ? [result.commandMessageId] : [])
    if (result.commandMessageId) anomalous.add(result.commandMessageId)
    for (const outcome of capturedOutcomes) if (outcome.commandMessageId === result.commandMessageId) unassociated.add(outcome.responseMessageId)
  }
  const edges: { parentMessageId: string; childMessageId: string }[] = []
  for (const { message } of messages.values()) {
    if (message.causationId && messages.has(message.causationId) && !anomalous.has(message.messageId) && !anomalous.has(message.causationId)) {
      edges.push({ parentMessageId: message.causationId, childMessageId: message.messageId })
    }
  }
  return { edges, anomalousMessageIds: [...anomalous], unassociatedOutcomeIds: [...unassociated] }
}
