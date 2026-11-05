import { byteLength, decimal, freeze, MAX_INPUT_BYTES, object, quote, readJson } from './json.ts'
import { EvidenceError } from './errors.ts'
import { checkObserved, checkTimeout } from './schema.ts'
import behavioralSchema from './schema/behavioral-test-v2.schema.json' with { type: 'json' }
import { analyseSeries, redactedRejection } from './analysis.ts'
import type { Diagnostic, HandlerSimulation, Json, JsonObject, Operation, OperationSeries, Presentation, ResolvedSelection, Selection } from './types.ts'

const own = (value: object, key: string) => Object.hasOwn(value, key)

function text(value: unknown, field: string): string {
  if (typeof value !== 'string' || value.trim() === '') throw new Error(`Invalid ${field}: expected nonempty text`)
  return value
}

function identifier(value: unknown, field: string): string {
  const result = text(value, field)
  if (!/^[\x21-\x7e]{1,256}$/.test(result)) throw new Error(`Invalid ${field}: expected a canonical identifier`)
  return result
}

function choice(value: unknown, choices: readonly string[], field: string): string {
  if (typeof value !== 'string' || !choices.includes(value)) throw new Error(`Unsupported ${field}`)
  return value
}

function uint(value: unknown, field: string, minimum = 0, maximum = Number.MAX_SAFE_INTEGER): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < minimum || value > maximum) throw new Error(`Invalid ${field}`)
  return value
}

function array(value: unknown, field: string): readonly Json[] {
  if (!Array.isArray(value)) throw new Error(`Invalid ${field}: expected an array`)
  return value
}

function noObservationScope(value: JsonObject) {
  if (['scope', 'application', 'generation'].some((key) => own(value, key))) {
    throw new Error('Unsupported observation scope: pass an operation and its finite capture')
  }
}

function keys(value: JsonObject, allowed: readonly string[], path: string) {
  if (Object.keys(value).some((key) => !allowed.includes(key))) throw new EvidenceError('INVALID_CAPTURE', 'Unsupported field in canonical section', path)
}

function operationOf(value: Json): Operation {
  const op = object(value, 'operation')
  noObservationScope(op)
  for (const key of ['operationId', 'correlationId']) identifier(op[key], `operation.${key}`)
  choice(op.mode, ['test', 'dispatch'], 'operation mode (legacy simulate is unsupported)')
  choice(op.status, ['queued', 'running', 'completed', 'failed', 'indeterminate'], 'operation status')
  text(op.context, 'operation.context')
  text(op.command, 'operation.command')
  uint(op.schemaVersion, 'operation.schemaVersion', 1, 4294967295)
  if (op.events !== undefined && object(op.events, 'operation.events').kind !== 'observed') {
    throw new Error('Unsupported predicted operation evidence')
  }
  if (op.result !== undefined) {
    const result = object(op.result, 'operation.result')
    choice(result.decision, ['accepted', 'rejected'], 'operation decision')
    if (result.published !== true) throw new Error('Operation result must record publication; use distinct handlerSimulation evidence for proposals')
    for (const key of ['commandMessageId', 'responseMessageId']) if (result[key] !== undefined) identifier(result[key], `result.${key}`)
    if (result.decision === 'rejected' && !own(result, 'rejection')) throw new Error('Missing operation rejection')
  }
  if (op.failure !== undefined) object(op.failure, 'operation.failure')
  return { ...op, latestEventId: decimal(op.latestEventId, 'operation.latestEventId') } as unknown as Operation
}

function simulationOf(source: JsonObject): HandlerSimulation {
  if (source.committed !== false) throw new Error('Handler simulation must be explicitly uncommitted')
  if (['operationId', 'correlationId', 'operation', 'messageSeries', 'capture', 'report', 'runState'].some((key) => own(source, key))) {
    throw new Error('Handler simulation cannot contain operation or observed capture evidence')
  }
  keys(source, ['kind', 'committed', 'context', 'command', 'schemaVersion', 'outcome', 'streams', 'proposedEvents'], '')
  text(source.context, 'simulation.context')
  text(source.command, 'simulation.command')
  uint(source.schemaVersion, 'simulation.schemaVersion', 1, 4294967295)
  const outcome = object(source.outcome, 'simulation.outcome')
  keys(outcome, ['decision', 'rejection'], '/outcome')
  choice(outcome.decision, ['accepted', 'rejected'], 'simulation decision')
  const streams = array(source.streams, 'simulation.streams').map((item) => {
    const stream = object(item, 'simulation stream')
    keys(stream, ['aggregateType', 'aggregateId', 'baseStreamVersion', 'readGuard'], '/streams')
    text(stream.aggregateType, 'stream.aggregateType')
    text(stream.aggregateId, 'stream.aggregateId')
    if (typeof stream.readGuard !== 'boolean') throw new Error('Invalid stream.readGuard')
    return { ...stream, baseStreamVersion: decimal(stream.baseStreamVersion, 'stream.baseStreamVersion') }
  })
  const proposedEvents = array(source.proposedEvents, 'simulation.proposedEvents').map((item) => {
    const event = object(item, 'proposed event')
    keys(event, ['aggregateType', 'aggregateId', 'eventType', 'schemaVersion', 'proposedStreamVersion', 'payload'], '/proposedEvents')
    for (const key of ['aggregateType', 'aggregateId', 'eventType']) text(event[key], `proposal.${key}`)
    uint(event.schemaVersion, 'proposal.schemaVersion', 1, 4294967295)
    return { ...event, proposedStreamVersion: decimal(event.proposedStreamVersion, 'proposal.proposedStreamVersion') }
  })
  if (streams.length > 4096 || proposedEvents.length > 4096) throw new EvidenceError('INPUT_LIMIT', 'Simulation exceeds the 4096-record presentation bound')
  if (outcome.decision === 'rejected' && (!own(outcome, 'rejection') || proposedEvents.length !== 0)) {
    throw new Error('Rejected simulation requires rejection and no event proposals')
  }
  return { ...source, streams, proposedEvents } as unknown as HandlerSimulation
}

function seriesOf(value: JsonObject, op: Operation, path: string): OperationSeries {
  noObservationScope(value)
  keys(value, ['operationId', 'correlationId', 'mode', 'messageSeries', 'capture'], path)
  for (const key of ['operationId', 'correlationId', 'mode'] as const) {
    if (value[key] !== op[key]) throw new EvidenceError('INVALID_CAPTURE', `Operation/capture identity mismatch: ${key}`, `${path}/${key}`)
  }
  const capture = object(value.capture, 'capture')
  keys(capture, ['settled', 'settledFor', 'fidelity', 'note'], `${path}/capture`)
  if (typeof capture.settled !== 'boolean') throw new Error('Invalid capture.settled')
  checkTimeout(capture.settledFor, `${path}/capture/settledFor`)
  choice(capture.fidelity, ['exact', 'grouped'], 'capture fidelity')
  if (capture.note !== undefined && typeof capture.note !== 'string') throw new Error('Invalid capture.note')
  const series = object(value.messageSeries, 'messageSeries')
  checkObserved(series, `${path}/messageSeries`)
  return value as unknown as OperationSeries
}

const views = new WeakSet<object>()
export function assertPresentation(view: Presentation) {
  if (!views.has(view)) throw new EvidenceError('INVALID_VIEW', 'Expected an immutable reader-produced presentation')
}
function finish<T extends Presentation>(view: T): T {
  freeze(view)
  views.add(view)
  return view
}

function provenanceOf(source: JsonObject, diagnostics: Diagnostic[]): 'supported' | 'unknown' {
  const supported: Record<string, number> = { catalog: 1, behavioralTest: 2, messageSeries: 2, fixture: 1 }
  let complete = true
  for (const releaseName of ['release', 'cliRelease']) {
    const release = source[releaseName]
    if (!release || typeof release !== 'object' || Array.isArray(release) || !('contracts' in release)) { complete = false; continue }
    const contracts = object(release.contracts, `/${releaseName}/contracts`)
    for (const [name, version] of Object.entries(supported)) {
      if (contracts[name] === undefined) complete = false
      else if (contracts[name] !== version) throw new EvidenceError('UNSUPPORTED_CONTRACT', 'Unsupported or disagreeing declared contract version', `/${releaseName}/contracts/${name}`)
    }
  }
  for (const [value, expected, path] of [
    [source.catalogVersion, 1, '/catalogVersion'],
    [source.definition && object(source.definition, 'definition').schemaVersion, 2, '/definition/schemaVersion'],
    [source.fixture && object(source.fixture, 'fixture').schemaVersion, 1, '/fixture/schemaVersion'],
  ] as const) {
    if (value !== undefined && value !== expected) throw new EvidenceError('UNSUPPORTED_CONTRACT', 'Unsupported declared contract version', path)
  }
  if (source.schema !== undefined) {
    const schema = object(source.schema, '/schema')
    if (schema.$id !== undefined && schema.$id !== behavioralSchema.$id) throw new EvidenceError('UNSUPPORTED_CONTRACT', 'Unsupported embedded behavioral schema identity', '/schema/$id')
  }
  if (!complete) diagnostics.push({ code: 'PROVENANCE_UNKNOWN', severity: 'warning', path: '', message: 'Release contracts are absent or incomplete; this capture is checked against the bundled canonical v2 schema only.' })
  if (source.release && source.cliRelease) {
    const release = object(source.release, 'release')
    const cli = object(source.cliRelease, 'cliRelease')
    for (const key of ['frameworkVersion', 'sourceRevision', 'sourceFingerprint']) {
      if (release[key] !== undefined && cli[key] !== undefined && release[key] !== cli[key]) diagnostics.push({ code: 'PROVENANCE_DISAGREEMENT', severity: 'warning', path: `/release/${key}`, relatedPaths: [`/cliRelease/${key}`], message: 'Release provenance differs; matching contract versions do not establish matching source builds.' })
    }
  }
  return complete ? 'supported' : 'unknown'
}

/** Open saved CLI JSON, a snapshot plus optional finite capture, or a simulation. */
export function readEvidence(input: unknown, finiteCapture?: unknown): Presentation {
  const source = readJson(input)
  noObservationScope(source)
  const warnings: string[] = []
  const diagnostics: Diagnostic[] = []
  const sourceText = typeof input === 'string' ? input : JSON.stringify(source)
  if (byteLength(sourceText) > MAX_INPUT_BYTES) throw new EvidenceError('INPUT_LIMIT', 'Evidence exceeds the 8 MiB serialized-input budget')
  const provenance = provenanceOf(source, diagnostics)
  if (typeof input !== 'string') diagnostics.push({ code: 'SOURCE_LEXEMES_UNAVAILABLE', severity: 'warning', path: '', message: 'Caller supplied parsed objects; sourceText preserves that value, not the original file bytes.' })
  if (finiteCapture !== undefined && typeof finiteCapture !== 'string') diagnostics.push({ code: 'SOURCE_LEXEMES_UNAVAILABLE', severity: 'warning', path: '/captureInput', message: 'Caller supplied a parsed capture; captureText cannot recover its original numeric tokens or file bytes.' })
  if (typeof finiteCapture === 'string' && byteLength(sourceText) + byteLength(finiteCapture) > MAX_INPUT_BYTES) throw new EvidenceError('INPUT_LIMIT', 'Combined evidence exceeds the 8 MiB input budget')
  const base = { source, sourceText, snapshotId: globalThis.crypto.randomUUID(), provenance, diagnostics, warnings }
  if (source.kind === 'handlerSimulation') {
    if (finiteCapture !== undefined) throw new Error('Handler simulation cannot have a capture')
    const simulation = simulationOf(source)
    warnings.push('Handler simulation is uncommitted; proposals are not observed events or a durable decision.')
    if (redactedRejection(simulation.outcome.rejection)) warnings.push('Simulation rejection details are redacted in the saved evidence.')
    if (simulation.proposedEvents.some((event) => !own(event, 'payload'))) warnings.push('Proposal payload is missing or redacted; no replacement is inferred.')
    return finish({ ...base, kind: 'handlerSimulation', simulation })
  }
  let operation: Operation
  let report: JsonObject | undefined
  let runState: string | undefined
  let captureState: string | undefined
  let capture: JsonObject | undefined
  let captureText: string | undefined
  if (own(source, 'runState')) {
    if (finiteCapture !== undefined) throw new Error('Saved evidence already defines its capture')
    runState = choice(source.runState, ['not-submitted', 'submission-outcome-unknown', 'response-received', 'reported'], 'runState')
    if (source.error !== undefined) warnings.push('Saved evidence contains an execution/retrieval error; inspect source.error for details.')
    if (source.captureState !== undefined) captureState = choice(source.captureState, ['complete', 'incomplete'], 'captureState')
    if (source.report !== undefined) report = object(source.report, 'report')
    if (!report || !own(report, 'operation')) {
      if (source.messageSeries !== undefined) throw new Error('Missing operation for saved capture')
      warnings.push('Partial saved evidence: no operation was recorded; submission status is not a business decision.')
      return finish({ ...base, kind: 'incomplete', runState })
    }
    operation = operationOf(report.operation)
    if (operation.mode !== 'test') throw new Error('Behavioral evidence requires Test operation mode')
    choice(report.status, ['passed', 'failed'], 'behavioral status')
    for (const key of ['operationId', 'correlationId'] as const) {
      if (report[key] !== operation[key]) throw new Error(`Report/operation identity mismatch: ${key}`)
    }
    // Invalid capture shape is diagnosed below, independently of the report.
    if (runState !== 'reported' || captureState !== 'complete') warnings.push('Partial saved retrieval; runState and captureState do not establish business completion.')
  } else {
    operation = operationOf(source)
  }
  const capturePath = runState ? '/messageSeries' : '/captureInput'
  let series: OperationSeries | undefined
  let captureStatus: 'valid' | 'anomalous' | 'invalid' | 'missing' = 'missing'
  if ((runState && source.messageSeries !== undefined) || finiteCapture !== undefined) {
    try {
      capture = runState ? object(source.messageSeries, 'finite capture') : readJson(finiteCapture)
      captureText = runState ? undefined : typeof finiteCapture === 'string' ? finiteCapture : JSON.stringify(capture)
      if (byteLength(sourceText) + (captureText ? byteLength(captureText) : 0) > MAX_INPUT_BYTES) throw new EvidenceError('INPUT_LIMIT', 'Combined evidence exceeds the 8 MiB input budget')
      series = seriesOf(capture, operation, capturePath)
      captureStatus = 'valid'
    } catch (error) {
      if (error instanceof EvidenceError && ['INPUT_LIMIT', 'DEPTH_LIMIT'].includes(error.code)) throw error
      if (!runState && typeof finiteCapture !== 'string' && capture === undefined) throw error
      captureStatus = 'invalid'
      diagnostics.push({ code: 'INVALID_CAPTURE', severity: 'error', path: error instanceof EvidenceError ? error.path : capturePath, message: `${error instanceof EvidenceError ? error.message + '. ' : ''}Finite capture failed canonical validation; source retained, causal indexing and selection withheld.` })
      // Text for a malformed second input must remain available too.
      if (!runState && typeof finiteCapture === 'string') captureText = finiteCapture
    }
  }
  if (redactedRejection(operation.result?.rejection)) warnings.push('Operation rejection details are redacted in the saved evidence.')
  if (!series) {
    warnings.push(captureStatus === 'invalid' ? 'Finite capture is invalid; independent report/operation facts are retained.' : 'Finite capture is missing; report observations and expectations are not substituted.')
  } else {
    warnings.push('Finite capture only; retrieval and settling do not prove downstream business completion.')
    if (!series.capture.settled) warnings.push('Partial capture: the settling condition was not met.')
    if (series.capture.fidelity === 'grouped') warnings.push('Grouped capture: causal detail is limited; only explicit matching causation IDs form edges.')
    for (const message of series.messageSeries.messages) {
      if (!own(message, 'payload')) warnings.push(`Message ${quote(message.messageId)} payload is missing or redacted; no replacement is inferred.`)
    }
    for (const outcome of series.messageSeries.commandOutcomes) {
      if (redactedRejection(outcome.outcome.value)) warnings.push(`Outcome ${quote(outcome.responseMessageId)} rejection details are redacted in the saved evidence.`)
    }
  }
  const analysis = analyseSeries(series, operation, report, diagnostics, capturePath)
  if (series && diagnostics.some((item) => item.severity === 'error')) captureStatus = 'anomalous'
  return finish({ ...base, kind: 'operation', operation, series, ...analysis, captureStatus, captureText, report, runState, captureState })
}

/** Bind an explicit message choice to this open snapshot. Never rebind a callback. */
export function createSelection(view: Presentation, messageId: string): Selection {
  assertPresentation(view)
  if (view.kind !== 'operation') throw new EvidenceError('INVALID_SELECTION', 'Selection requires operation evidence')
  const selection = freeze({ snapshotId: view.snapshotId, operationId: view.operation.operationId, messageId })
  resolveSelection(view, selection)
  return selection
}

/** Resolve exact identities against this detached snapshot, never a global cache. */
export function resolveSelection(view: Presentation, selection: Selection): ResolvedSelection {
  assertPresentation(view)
  if (!selection || typeof selection.operationId !== 'string' || typeof selection.messageId !== 'string') throw new EvidenceError('INVALID_SELECTION', 'Selection requires exact operation and message identities')
  if (view.kind !== 'operation' || selection.operationId !== view.operation.operationId) {
    throw new EvidenceError('STALE_SELECTION', 'Selection operation does not match this saved evidence')
  }
  if (selection.snapshotId !== view.snapshotId) throw new EvidenceError('STALE_SELECTION', 'Selection snapshot does not match this immutable saved view')
  if (!view.series) throw new EvidenceError('CAPTURE_UNAVAILABLE', 'Selection cannot resolve: finite capture is missing or invalid')
  const message = view.series.messageSeries.messages.find((item) => item.messageId === selection.messageId)
  if (!message) throw new EvidenceError('INVALID_SELECTION', 'Selected message is missing from this saved evidence')
  const outcome = message.kind === 'command'
    ? view.series.messageSeries.commandOutcomes.find((item) => item.commandMessageId === message.messageId && !view.unassociatedOutcomeIds.includes(item.responseMessageId))
    : undefined
  const index = view.series.messageSeries.messages.indexOf(message)
  const diagnostics = view.diagnostics.filter((item) => item.messageIds?.includes(message.messageId))
  return freeze({ snapshotId: view.snapshotId, operationId: view.operation.operationId, message, outcome, diagnostics, path: `${view.runState ? '/messageSeries' : '/captureInput'}/messageSeries/messages/${index}` })
}
