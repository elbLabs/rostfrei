import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile, mkdtemp, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import * as presentation from '../src/index.ts'
import { readEvidence, createSelection, renderText, resolveSelection } from '../src/index.ts'
import { savedEvidence, simulation } from './fixtures.mjs'

test('Mermaid presents the saved outcome separately from the verdict and only reader-approved edges', async () => {
  const rejected = readEvidence(await readFile(new URL('./canonical/rejected.json', import.meta.url), 'utf8'))
  const summary = presentation.summarizeEvidence(rejected)
  assert.equal(summary.facts.find((fact) => fact.label === 'Command outcome').value, 'rejected')
  assert.equal(summary.facts.find((fact) => fact.label === 'Test verdict').value, 'passed')
  assert.equal(summary.title, 'rent-bicycle')
  assert.equal(summary.facts.find((fact) => fact.label === 'Capture').value, 'settled')
  const graph = presentation.renderMermaid(rejected)
  assert.match(graph, /Command outcome: rejected/)
  assert.match(graph, /Test verdict: passed/)
  assert.match(graph, /Command outcome: rejected<br\/>Test verdict: passed<br\/>Capture: settled/)
  assert.doesNotMatch(graph, /·/)
  assert.equal((graph.match(/^  n\d+\[/gm) ?? []).length, 1)
  assert.doesNotMatch(graph, /-->/)
  assert.doesNotMatch(graph, /bike-99/, 'Payloads stay out of diagram labels')
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.messages[1].name = 'rental · confirmed'
  saved.messageSeries.messageSeries.messages[1].causationId = 'integration-started'
  const anomalous = readEvidence(saved)
  assert.deepEqual(anomalous.edges, [])
  assert.doesNotMatch(presentation.renderMermaid(anomalous), /-->/, 'Do not reconstruct withheld cycle edges')
  assert.match(presentation.renderMermaid(anomalous), /anomalous/)
  assert.match(presentation.renderMermaid(anomalous), /rental · confirmed/, 'Punctuation inside recorded labels is preserved')
  assert.throws(() => presentation.renderMermaid({ ...anomalous }), /reader-produced/)
})

test('Mermaid bounds nodes and labels with explicit omissions, keeps partial evidence and treats labels as inert data', () => {
  const saved = savedEvidence()
  saved.messageSeries.capture = { settled: false, settledFor: '500ms', fidelity: 'grouped' }
  saved.messageSeries.messageSeries.messages[0].name = 'evil"]\nclick n0 "javascript:alert(1)"\n%%{init:{}}%% <img> ` [&|#;\\\u202e'
  delete saved.messageSeries.messageSeries.messages[0].payload
  saved.messageSeries.messageSeries.messages[1].name = 'long'.repeat(100)
  const view = readEvidence(saved)
  const graph = presentation.renderMermaid(view, { maxNodes: 2 })
  assert.equal((graph.match(/^  n\d+\[/gm) ?? []).length, 2)
  assert.equal((graph.match(/-->/g) ?? []).length, 1)
  assert.match(graph, /Omitted 1 message and 1 causal edge/)
  assert.match(graph, /partial.*grouped/)
  assert.match(graph, /missing or redacted/)
  assert.match(graph, /label clipped/)
  assert.doesNotMatch(graph, /\nclick|%%\{|<img>|\u202e/)
  assert.match(graph, /#34;/)
  assert.throws(() => presentation.renderMermaid(view, { maxNodes: 0 }), /node budget/i)
  assert.throws(() => presentation.renderMermaid(readEvidence(simulation())), /uncommitted/i)
  delete saved.messageSeries
  assert.throws(() => presentation.renderMermaid(readEvidence(saved)), /capture.*missing/i)
})

test('summary is a bounded wire-safe projection without source payloads; conflicting decisions stay qualified', async () => {
  const saved = savedEvidence()
  saved.report.operation.result.decision = 'rejected'
  saved.report.operation.result.rejection = { redacted: true }
  const view = readEvidence(saved)
  const summary = presentation.summarizeEvidence(view)
  assert.deepEqual(JSON.parse(JSON.stringify(summary)), summary)
  assert.match(summary.facts.find((fact) => fact.label === 'Command outcome').value, /conflicting/)
  assert.match(summary.limitations.join(' '), /redacted/)
  assert.ok(summary.diagnostics.some((line) => line.includes('OUTCOME_CONFLICT')))
  assert.ok(summary.issues.includes('Recorded command outcomes conflict.'))
  assert.equal(summary.issues.filter((line) => line === 'Recorded command outcomes conflict.').length, 1)
  assert.doesNotMatch(summary.issues.join('\n'), /OUTCOME_CONFLICT|\/report|command-rent/)
  for (const [change, issue] of [
    [(source) => { source.messageSeries.messageSeries.messages[1].causationId = 'missing-parent' }, 'A parent message is missing from the capture.'],
    [(source) => { source.messageSeries.operationId = 'wrong-operation' }, 'Capture unavailable: validation failed.'],
  ]) {
    const source = savedEvidence()
    change(source)
    assert.ok(presentation.summarizeEvidence(readEvidence(source)).issues.includes(issue))
  }
  const canonical = readEvidence(await readFile(new URL('./canonical/accepted.json', import.meta.url), 'utf8'))
  assert.deepEqual(presentation.summarizeEvidence(canonical).issues, [])
  assert.doesNotMatch(JSON.stringify(summary), /bike-42|"sourceText":|"payload":/)
  assert.throws(() => presentation.summarizeEvidence({ ...view }), /reader-produced/)
})

test('file adapter exports Markdown or Mermaid offline, retains exact input and refuses overwrites', async () => {
  const scratch = await mkdtemp('/tmp/opencode/evidence-export-').catch((error) => {
    if (!['EACCES', 'ENOENT'].includes(error.code)) throw error
    return mkdtemp(join(tmpdir(), 'evidence-export-'))
  })
  const input = `${scratch}/input.json`
  const output = `${scratch}/run.md`
  const saved = savedEvidence()
  saved.messageSeries.messageSeries.messages[0].payload = { exact: 'NUMBER_PLACEHOLDER' }
  const raw = JSON.stringify(saved).replace('"NUMBER_PLACEHOLDER"', '18446744073709551615')
  await writeFile(input, raw)
  const cli = new URL('../bin/render.mjs', import.meta.url)
  const run = (...args) => spawnSync(process.execPath, [cli.pathname, ...args], { encoding: 'utf8' })
  const rendered = run(input, '--output', output)
  assert.equal(rendered.status, 0, rendered.stderr)
  const markdown = await readFile(output, 'utf8')
  assert.match(markdown, /Command outcome: accepted/)
  assert.match(markdown, /```mermaid\nflowchart TD/)
  assert.match(markdown, /<details>/)
  assert.match(markdown, /18446744073709551615/)
  assert.equal(await readFile(input, 'utf8'), raw)
  assert.notEqual(run(input, '--output', input).status, 0)
  assert.notEqual(run(input, '--output', output).status, 0)
  const mermaid = run(input, '--format', 'mermaid')
  assert.equal(mermaid.status, 0, mermaid.stderr)
  assert.match(mermaid.stdout, /^flowchart TD/)
  assert.ok(mermaid.stdout.includes(`\n%% SHA-256: ${createHash('sha256').update(await readFile(input)).digest('hex')}\n`), 'Raw Mermaid retains the exact input byte digest as an inert comment')
})

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
    assert.equal(presentation.renderMermaid(view), presentation.renderMermaid(view))
    assert.deepEqual(presentation.summarizeEvidence(view), presentation.summarizeEvidence(view))
    resolveSelection(view, createSelection(view, 'command-rent'))
  } finally {
    globalThis.fetch = originalFetch
    Date.now = originalNow
  }
})
