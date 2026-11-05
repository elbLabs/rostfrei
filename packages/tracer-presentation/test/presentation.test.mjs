import assert from 'node:assert/strict'
import test from 'node:test'
import { readEvidence, createSelection, renderText, resolveSelection } from '../src/index.ts'
import { savedEvidence, simulation } from './fixtures.mjs'

test('saved CLI envelope and operation/capture pair present the same observed identities', () => {
  const saved = savedEvidence()
  const view = readEvidence(saved)
  const pair = readEvidence(saved.report.operation, saved.messageSeries)
  assert.equal(view.kind, 'operation')
  assert.deepEqual(view.edges, pair.edges)
  assert.deepEqual(view.edges, [
    { parentMessageId: 'command-rent', childMessageId: 'event-rented' },
    { parentMessageId: 'event-rented', childMessageId: 'integration-started' },
  ])
  assert.equal(view.source.fixture.revision, saved.fixture.revision)
  const selection = resolveSelection(view, createSelection(view, 'command-rent'))
  assert.equal(selection.message.messageId, 'command-rent')
  assert.equal(selection.outcome.responseMessageId, 'response-1')
  assert.equal(view.series.messageSeries.messages.length, 3)
})

test('cross-operation stale selection fails even when message IDs overlap', () => {
  const saved = savedEvidence()
  saved.report.operation.operationId = saved.report.operationId = saved.messageSeries.operationId = 'operation-2'
  const view = readEvidence(saved)
  assert.throws(() => resolveSelection(view, { operationId: 'operation-1', messageId: 'command-rent' }), /operation/i)
  assert.throws(() => renderText(view, { selection: { operationId: 'operation-1', messageId: 'command-rent' } }), /operation/i)
  assert.throws(() => createSelection(view, 'absent'), /missing/i)
})

test('IDs are exact opaque strings, including prototype-like keys', () => {
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.messages[0].messageId = '__proto__'
  saved.messageSeries.messageSeries.messages[1].causationId = '__proto__'
  saved.messageSeries.messageSeries.commandOutcomes[0].commandMessageId = '__proto__'
  saved.report.operation.result.commandMessageId = '__proto__'
  saved.report.observed.commandOutcomes[0].commandMessageId = '__proto__'
  const view = readEvidence(saved)
  assert.equal(view.edges[0].parentMessageId, '__proto__')
  assert.equal(resolveSelection(view, createSelection(view, '__proto__')).message.messageId, '__proto__')
  assert.throws(() => resolveSelection(view, { operationId: 'operation-1 ', messageId: '__proto__' }), /operation/i)
})

test('missing parents stay disconnected; expected graphs, correlation and order do not create edges', () => {
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.messages[1].causationId = 'missing-parent'
  delete saved.messageSeries.messageSeries.messages[2].causationId
  const view = readEvidence(saved)
  assert.deepEqual(view.edges, [])
  assert.match(renderText(view), /missing parent/i)
  assert.ok(!renderText(view).includes('command-rent ->'))
})

test('passed suite with rejected decision and zero events is explicitly rejected', () => {
  const saved = savedEvidence()
  saved.report.operation.result = {
    decision: 'rejected', published: true, commandMessageId: 'command-rent',
    responseMessageId: 'response-1', rejection: { code: 'BICYCLE_UNAVAILABLE', payload: { bicycle_id: 'bike-99' } },
  }
  saved.messageSeries.messageSeries.messages.splice(1)
  saved.messageSeries.messageSeries.commandOutcomes[0].observationOrder = 1
  saved.messageSeries.messageSeries.commandOutcomes[0].outcome = {
    status: 'rejected', value: { classification: 'conflict', code: 'BICYCLE_UNAVAILABLE', message: 'Unavailable', details: { bicycle_id: 'bike-99' } },
  }
  const view = readEvidence(saved)
  const text = renderText(view)
  assert.match(text, /Behavioral test: passed/)
  assert.match(text, /business decision: rejected/)
  assert.match(text, /BICYCLE_UNAVAILABLE/)
  assert.match(text, /Captured events: 0/)
  assert.ok(!text.includes('bike-99'))
  assert.match(renderText(view, { includePayloads: true }), /bike-99/)
  delete saved.report.operation.result
  saved.messageSeries.messageSeries.commandOutcomes = []
  const unknown = renderText(readEvidence(saved))
  assert.match(unknown, /business decision: not recorded/)
  assert.ok(!unknown.includes('business decision: rejected'))
})

test('partial, grouped, missing/redacted payload and capture note are visible', () => {
  const saved = savedEvidence()
  saved.captureState = 'incomplete'
  saved.messageSeries.capture = { settled: false, settledFor: '500ms', fidelity: 'grouped', note: 'Retention limited' }
  delete saved.messageSeries.messageSeries.messages[1].payload
  const text = renderText(readEvidence(saved), { includePayloads: true })
  for (const warning of [/partial/i, /grouped/i, /missing or redacted/i, /Retention limited/]) assert.match(text, warning)
})

test('missing capture does not silently substitute report.observed or expected nodes', () => {
  const saved = savedEvidence()
  delete saved.messageSeries
  saved.captureState = 'incomplete'
  const view = readEvidence(saved)
  assert.deepEqual(view.edges, [])
  assert.match(renderText(view), /finite capture is missing/i)
  assert.throws(() => createSelection(view, 'command-rent'), /capture/i)
})

test('uncertain submission remains inspectable without fabricated operation identity', () => {
  const saved = savedEvidence()
  delete saved.report
  delete saved.messageSeries
  delete saved.captureState
  saved.runState = 'submission-outcome-unknown'
  const view = readEvidence(saved)
  assert.equal(view.kind, 'incomplete')
  assert.match(renderText(view), /submission-outcome-unknown/)
  assert.throws(() => resolveSelection(view, { operationId: 'operation-1', messageId: 'command-rent' }), /operation/i)
})

test('simulation remains an uncommitted proposal without invented operation/messages/edges', () => {
  const view = readEvidence(simulation())
  assert.equal(view.kind, 'handlerSimulation')
  assert.equal(view.simulation.streams[0].baseStreamVersion, '18446744073709551614')
  assert.equal(view.simulation.proposedEvents[0].proposedStreamVersion, '18446744073709551615')
  assert.ok(!('operation' in view))
  assert.ok(!('edges' in view))
  assert.match(renderText(view), /uncommitted/i)
  assert.ok(!renderText(view).includes('bike-42'))
  assert.throws(() => resolveSelection(view, { operationId: 'operation-1', messageId: 'command-rent' }), /operation/i)
  assert.throws(() => readEvidence({ ...simulation(), committed: true }), /uncommitted/i)
  assert.throws(() => readEvidence({ ...simulation(), operationId: 'operation-1' }), /simulation/i)
  assert.throws(() => readEvidence({ ...simulation(), outcome: { decision: 'rejected', rejection: {} } }), /proposal/i)
})

test('canonical Rust numeric u64 metadata is read losslessly from raw JSON', () => {
  const raw = JSON.stringify(simulation()).replace('"18446744073709551614"', '18446744073709551614').replace('"18446744073709551615"', '18446744073709551615')
  const view = readEvidence(raw)
  assert.equal(view.simulation.streams[0].baseStreamVersion, '18446744073709551614')
  assert.equal(view.simulation.proposedEvents[0].proposedStreamVersion, '18446744073709551615')
  assert.match(renderText(view), /18446744073709551615/)
  assert.throws(() => readEvidence(JSON.parse(raw)), /unsafe/i)
})

test('payloads are opt-in, preserve explicit null and large raw JSON numbers', () => {
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.messages[0].payload = null
  saved.messageSeries.messageSeries.messages[1].payload = { secret: 'payload-only-marker' }
  const raw = JSON.stringify(saved).replace('"payload-only-marker"', '18446744073709551615')
  const view = readEvidence(raw)
  assert.ok(!renderText(view).includes('18446744073709551615'))
  const text = renderText(view, { includePayloads: true })
  assert.match(text, /payload: null/)
  assert.match(text, /18446744073709551615/)
})

test('hostile names, IDs, notes and payload strings cannot inject terminal or markup content', () => {
  const saved = savedEvidence()
  const hostile = '<script>**[x](javascript:alert(1))**`\u001b[31m\nforged\u202e'
  saved.messageSeries.messageSeries.messages[1].name = hostile
  saved.messageSeries.messageSeries.messages[1].payload = { [hostile]: hostile }
  saved.messageSeries.messageSeries.messages[2].messageId = '<script>__[x]'
  saved.messageSeries.capture.note = hostile
  const text = renderText(readEvidence(saved), { includePayloads: true })
  assert.ok(!text.includes('<script>'))
  assert.ok(!text.includes('\u001b'))
  assert.ok(!text.includes('\u202e'))
  assert.ok(!text.includes('\nforged'))
  assert.ok(!text.includes('[x]'))
  assert.ok(!text.includes('**'))
  assert.match(text, /\\u001b/)
  assert.match(text, /\\u202e/)
  const view = readEvidence(saved)
  assert.equal(resolveSelection(view, createSelection(view, '<script>__[x]')).message.messageId, '<script>__[x]')
})

test('evidence is detached and deeply immutable; rendering/selection are read-only', () => {
  const saved = savedEvidence()
  const before = JSON.stringify(saved)
  const view = readEvidence(saved)
  const first = renderText(view)
  resolveSelection(view, createSelection(view, 'event-rented'))
  assert.equal(JSON.stringify(saved), before)
  assert.equal(Object.isFrozen(saved), false)
  saved.messageSeries.messageSeries.messages[1].name = 'changed-after-open'
  assert.equal(renderText(view), first)
  assert.throws(() => { view.series.messageSeries.messages[1].name = 'mutation' }, TypeError)
  assert.throws(() => { view.source.fixture.messages.push({}) }, TypeError)
  assert.throws(() => { view.edges.push({}) }, TypeError)
})

test('rendering is deterministic across object key ordering and selected-message inspection', () => {
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.messages[0].payload = { z: 1, a: 2 }
  const first = renderText(readEvidence(saved), { includePayloads: true })
  saved.messageSeries.messageSeries.messages[0].payload = { a: 2, z: 1 }
  assert.equal(renderText(readEvidence(saved), { includePayloads: true }), first)
  const view = readEvidence(saved)
  const selected = renderText(view, { selection: createSelection(view, 'command-rent') })
  assert.match(selected, /command-rent/)
  assert.ok(!selected.includes('integration-started'))
})

test('operation/capture mismatches and ambiguous IDs withhold capture; report identity mismatch fails closed', () => {
  const saved = savedEvidence()
  assert.equal(readEvidence(saved.report.operation, { ...saved.messageSeries, operationId: 'operation-2' }).captureStatus, 'invalid')
  assert.equal(readEvidence(saved.report.operation, { ...saved.messageSeries, correlationId: 'correlation-2' }).captureStatus, 'invalid')
  assert.equal(readEvidence(saved.report.operation, { ...saved.messageSeries, mode: 'dispatch' }).captureStatus, 'invalid')
  assert.throws(() => readEvidence({ ...saved, report: { ...saved.report, operationId: 'operation-2' } }), /identity/i)
  saved.messageSeries.messageSeries.messages[1].messageId = 'command-rent'
  assert.equal(readEvidence(saved).captureStatus, 'invalid')
})

test('production observation scopes, bare series, and legacy simulate operations are unsupported', () => {
  const saved = savedEvidence()
  assert.throws(() => readEvidence({ application: 'app', scope: 'production', generation: '18446744073709551615', messageSeries: saved.messageSeries.messageSeries }), /unsupported/i)
  assert.throws(() => readEvidence(saved.messageSeries), /operation/i)
  assert.throws(() => readEvidence({ ...saved.report.operation, mode: 'simulate' }, { ...saved.messageSeries, mode: 'simulate' }), /simulate/i)
  assert.throws(() => readEvidence({ ...saved.report.operation, scope: 'production' }), /unsupported/i)
})

test('canonical identifier length and finite dispatch mode are preserved', () => {
  const saved = savedEvidence()
  saved.report.operation.operationId = saved.messageSeries.operationId = '1'.repeat(256)
  saved.report.operation.mode = saved.messageSeries.mode = 'dispatch'
  const view = readEvidence(saved.report.operation, saved.messageSeries)
  assert.equal(view.operation.operationId, '1'.repeat(256))
  assert.match(renderText(view), /mode: dispatch/)
  assert.throws(() => readEvidence({ ...saved.report.operation, operationId: '1'.repeat(257) }), /identifier/)
})

test('redacted rejection and orphan outcome retain their limitations', () => {
  const saved = savedEvidence()
  saved.report.operation.result = { decision: 'rejected', published: true, rejection: { redacted: true } }
  saved.messageSeries.messageSeries.commandOutcomes[0] = {
    commandMessageId: 'missing-command', responseMessageId: 'response-1',
    correlationId: 'correlation-1', observationOrder: 3,
    outcome: { status: 'rejected', value: { classification: 'internal', code: 'REDACTED', message: 'observed rejection redacted' } },
  }
  const view = readEvidence(saved)
  assert.match(renderText(view), /rejection details are redacted/i)
  assert.match(renderText(view), /missing command/i)
  assert.equal(resolveSelection(view, createSelection(view, 'command-rent')).outcome, undefined)
  assert.ok(view.diagnostics.some((item) => item.code === 'OUTCOME_CONFLICT'))
})

test('selection never associates an outcome with a non-command sharing its commandMessageId', () => {
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.commandOutcomes[0].commandMessageId = 'event-rented'
  const view = readEvidence(saved)
  const selection = createSelection(view, 'event-rented')
  assert.equal(resolveSelection(view, selection).outcome, undefined)
  assert.ok(!renderText(view, { selection }).includes('Command outcome:'))
  assert.match(renderText(view), /non-command; left unassociated/)
})

test('sparse input arrays are rejected without silently dropping evidence slots', () => {
  const saved = savedEvidence()
  delete saved.messageSeries.messageSeries.messages[1]
  assert.throws(() => readEvidence(saved), /dense JSON arrays/)
})

test('cross-correlation is diagnosed; malformed ordinals withhold capture rather than repairing it', () => {
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.messages[0].correlationId = 'different-correlation'
  assert.equal(readEvidence(saved).captureStatus, 'anomalous')
  saved.messageSeries.messageSeries.messages[0].correlationId = 'correlation-1'
  saved.messageSeries.messageSeries.messages[0].observationOrder = 2
  assert.equal(readEvidence(saved).captureStatus, 'invalid')
})

test('read/selection/render require no runtime network or clock', () => {
  const originalFetch = globalThis.fetch
  const originalNow = Date.now
  try {
    globalThis.fetch = () => { throw new Error('Unexpected network access') }
    Date.now = () => { throw new Error('Unexpected clock access') }
    const view = readEvidence(savedEvidence())
    assert.equal(renderText(view), renderText(view))
    resolveSelection(view, createSelection(view, 'command-rent'))
  } finally {
    globalThis.fetch = originalFetch
    Date.now = originalNow
  }
})
