import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import test from 'node:test'
import { readEvidence, createSelection, resolveSelection, renderText } from '../src/index.ts'

const raw = (name) => readFileSync(new URL(`./canonical/${name}.json`, import.meta.url), 'utf8')
const accepted = () => JSON.parse(raw('accepted'))
const rejected = () => JSON.parse(raw('rejected'))
const nested = (saved) => saved.messageSeries.messageSeries
const codes = (view) => view.diagnostics.map((item) => item.code)
const firstId = (view) => view.series.messageSeries.messages[0].messageId

test('untouched Rust acceptance/rejection and operation/capture pairs conform', () => {
  for (const [name, digest, decision, events] of [
    ['accepted', 'd99e755cd5f92bc3741685d04a89aba9db26de3deb2eab3d219533418dff38fb', 'accepted', 2],
    ['rejected', '3eaab73f65802130c3c80ff1cd42a98cb474832edae901391aad700e46778339', 'rejected', 0],
  ]) {
    const source = raw(name)
    assert.equal(createHash('sha256').update(source).digest('hex'), digest)
    const view = readEvidence(source)
    assert.equal(view.captureStatus, 'valid')
    assert.equal(view.provenance, 'supported')
    assert.equal(view.sourceText, source)
    assert.equal(view.report.status, 'passed')
    assert.equal(view.operation.result.decision, decision)
    assert.equal(view.edges.length, events)
    assert.ok(!view.diagnostics.some((item) => item.severity === 'error'))
    const record = resolveSelection(view, createSelection(view, firstId(view)))
    assert.equal(record.outcome.outcome.status, decision)
    const saved = JSON.parse(source)
    const pair = readEvidence(saved.report.operation, saved.messageSeries)
    assert.deepEqual(pair.edges, view.edges)
    assert.equal(pair.provenance, 'unknown')
    assert.match(renderText(view), new RegExp(`business decision: ${decision}`))
  }
  const outcome = readEvidence(raw('accepted')).series.messageSeries.commandOutcomes[0].outcome
  assert.equal(Object.hasOwn(outcome, 'value'), false)
})

test('accepted content allows absent/null but non-null degrades the capture', () => {
  const saved = accepted()
  nested(saved).commandOutcomes[0].outcome.value = null
  assert.equal(readEvidence(saved).captureStatus, 'valid')
  nested(saved).commandOutcomes[0].outcome.value = {}
  const view = readEvidence(saved)
  assert.equal(view.captureStatus, 'invalid')
  assert.equal(view.report.status, 'passed')
  assert.equal(view.series, undefined)
  assert.throws(() => createSelection(view, nested(saved).messages[0].messageId), /capture/i)
})

test('real recurring operation/event IDs cannot transfer selection across snapshots', () => {
  assert.equal(createHash('sha256').update(raw('earlier-accepted')).digest('hex'), 'e0faf34da8c68c192428771c02147e388b1663437f411e5d10aaa9d51728f6e3')
  const earlier = readEvidence(raw('earlier-accepted'))
  const later = readEvidence(raw('accepted'))
  const id = earlier.series.messageSeries.messages.find((message) => message.kind === 'domain-event').messageId
  const selection = createSelection(earlier, id)
  assert.equal(earlier.operation.operationId, later.operation.operationId)
  assert.notEqual(resolveSelection(earlier, selection).message.causationId, resolveSelection(later, createSelection(later, id)).message.causationId)
  assert.throws(() => resolveSelection(later, selection), (error) => error.code === 'STALE_SELECTION')
  assert.throws(() => renderText(later, { selection }), (error) => error.code === 'STALE_SELECTION')
  assert.throws(() => resolveSelection(earlier, { operationId: selection.operationId, messageId: id }), (error) => error.code === 'STALE_SELECTION')
  assert.equal(resolveSelection(earlier, selection).snapshotId, earlier.snapshotId)
  assert.match(renderText(earlier, { selection }), /domain-event/)
  assert.equal(resolveSelection(earlier, selection).message.messageId, id)
  assert.throws(() => resolveSelection(readEvidence(raw('earlier-accepted')), selection), /snapshot/i)
  assert.throws(() => renderText({ ...earlier }), /reader-produced/i)
})

test('fractional, underflow and exponent lexemes and original bytes survive exactly', () => {
  for (const lexeme of ['9007199254740990.5', '9007199254740990.1', '0.12345678901234567890123456789', '1e-400', '1e400', '-0', '1.00', '1E+03', '18446744073709551615']) {
    const saved = rejected()
    nested(saved).messages[0].payload = { probe: '__NUMBER__' }
    const source = JSON.stringify(saved, null, 2).replace('"__NUMBER__"', lexeme)
    const view = readEvidence(source)
    assert.equal(view.sourceText, source)
    assert.equal(JSON.stringify(view.source.messageSeries.messageSeries.messages[0].payload), `{"probe":${lexeme}}`)
    assert.ok(renderText(view, { includePayloads: true }).includes(`"probe":${lexeme}`))
  }
  const saved = rejected()
  saved.report.operation.latestEventId = '__CURSOR__'
  for (const lexeme of ['1e-400', '1.0', '1e2', '-0']) {
    assert.throws(() => readEvidence(JSON.stringify(saved).replace('"__CURSOR__"', lexeme)), /decimal/)
  }
})

test('canonical generated schemas remain byte-identical to Rust schema authority', () => {
  for (const name of ['observed-message-series-v2.schema.json', 'behavioral-test-v2.schema.json']) {
    assert.deepEqual(readFileSync(new URL(`../src/schema/${name}`, import.meta.url)), readFileSync(new URL(`../../../crates/rostfrei-tracer/schema/${name}`, import.meta.url)))
  }
})

test('schema/runtime-invalid capture sections degrade without losing report or source', () => {
  const mutations = [
    (s) => { nested(s).messages[0].aggregate = { type: 'FAKE', id: 'fake' } },
    (s) => { nested(s).unknown = 1 },
    (s) => { nested(s).commandOutcomes[0].outcome.value.classification = 'anything' },
    (s) => { nested(s).commandOutcomes[0].outcome.value.code = 'bad code!' },
    (s) => { nested(s).commandOutcomes[0].outcome.value.message = '\u0001control' },
    (s) => { nested(s).commandOutcomes[0].outcome.value.message = 'é'.repeat(513) },
    (s) => { s.messageSeries.capture.settledFor = 'forever' },
    (s) => { s.messageSeries.capture.settledFor = '60001ms' },
    (s) => { nested(s).messages[0].observationOrder = 5 },
  ]
  for (const mutate of mutations) {
    const saved = rejected()
    mutate(saved)
    const source = JSON.stringify(saved)
    const view = readEvidence(source)
    assert.equal(view.captureStatus, 'invalid')
    assert.equal(view.sourceText, source)
    assert.equal(view.report.status, 'passed')
    assert.equal(view.operation.result.decision, 'rejected')
    assert.equal(view.series, undefined)
    assert.deepEqual(view.edges, [])
    assert.ok(view.diagnostics.some((item) => item.code === 'INVALID_CAPTURE' && item.path.startsWith('/messageSeries')))
    assert.match(renderText(view), /INVALID_CAPTURE/)
  }
})

test('unsupported/mismatched declarations fail closed; absent provenance is explicit', () => {
  for (const mutate of [
    (s) => { s.release.contracts.messageSeries = s.cliRelease.contracts.messageSeries = 3 },
    (s) => { s.cliRelease.contracts.messageSeries = 1 },
    (s) => { s.catalogVersion = 2 },
    (s) => { s.definition.schemaVersion = 3 },
    (s) => { s.fixture.schemaVersion = 2 },
    (s) => { s.schema.$id = 'https://rostfrei.dev/schemas/tracer/behavioral-test-v3.schema.json' },
  ]) {
    const saved = rejected()
    mutate(saved)
    assert.throws(() => readEvidence(saved), (error) => error.code === 'UNSUPPORTED_CONTRACT' && typeof error.path === 'string')
  }
  const saved = rejected()
  delete saved.release
  delete saved.cliRelease
  assert.equal(readEvidence(saved).provenance, 'unknown')
  assert.ok(codes(readEvidence(saved)).includes('PROVENANCE_UNKNOWN'))
})

test('decisions, command/response identities and unredacted rejection details reconcile across sources', () => {
  for (const mutate of [
    (s) => { nested(s).commandOutcomes[0].outcome = { status: 'accepted' } },
    (s) => { nested(s).commandOutcomes[0].responseMessageId = 'another-response' },
    (s) => { s.report.commandOutcome.outcome = { status: 'accepted' } },
    (s) => { s.report.commandOutcome.commandMessageId = 'another-command' },
    (s) => { nested(s).commandOutcomes[0].commandMessageId = 'another-command' },
    (s) => { s.report.operation.result.commandMessageId = 'another-command' },
    (s) => { nested(s).commandOutcomes[0].outcome.value.details = { changed: true } },
  ]) {
    const saved = rejected()
    mutate(saved)
    const view = readEvidence(saved)
    assert.equal(view.captureStatus, 'anomalous')
    assert.ok(codes(view).includes('OUTCOME_CONFLICT'))
    const selected = resolveSelection(view, createSelection(view, firstId(view)))
    assert.equal(selected.outcome, undefined)
    assert.ok(selected.diagnostics.some((item) => item.code === 'OUTCOME_CONFLICT'))
    assert.match(renderText(view), /OUTCOME_CONFLICT/)
    assert.ok(view.diagnostics.find((item) => item.code === 'OUTCOME_CONFLICT').relatedPaths.length >= 1)
  }
  const saved = rejected()
  nested(saved).commandOutcomes[0].outcome.value = { classification: 'internal', code: 'REDACTED', message: 'observed rejection redacted' }
  assert.ok(!codes(readEvidence(saved)).includes('OUTCOME_CONFLICT'))
})

test('independent report conflicts remain diagnosed when the finite capture is missing', () => {
  const saved = rejected()
  delete saved.messageSeries
  saved.captureState = 'incomplete'
  saved.report.commandOutcome.outcome = { status: 'accepted' }
  const view = readEvidence(saved)
  assert.equal(view.captureStatus, 'missing')
  assert.ok(codes(view).includes('OUTCOME_CONFLICT'))
  assert.match(renderText(view), /business decision: rejected/)
  assert.match(renderText(view), /OUTCOME_CONFLICT/)
})

test('runtime rejection text accepts whitespace but raw JSON cannot bypass string escaping', () => {
  const saved = rejected()
  nested(saved).commandOutcomes[0].outcome.value.message = ' '
  const view = readEvidence(saved)
  assert.notEqual(view.captureStatus, 'invalid')
  nested(saved).messages[0].payload = JSON.rawJSON('"<script>"')
  assert.throws(() => readEvidence(saved), (error) => error.code === 'INVALID_JSON')
})

test('separate capture source retention and parsed-origin diagnostics are section-specific', () => {
  const saved = rejected()
  const operation = JSON.stringify(saved.report.operation, null, 2)
  const capture = JSON.stringify(saved.messageSeries, null, 4)
  const exact = readEvidence(operation, capture)
  assert.equal(exact.sourceText, operation)
  assert.equal(exact.captureText, capture)
  assert.ok(!codes(exact).includes('SOURCE_LEXEMES_UNAVAILABLE'))
  const parsed = readEvidence(operation, saved.messageSeries)
  assert.ok(parsed.diagnostics.some((item) => item.code === 'SOURCE_LEXEMES_UNAVAILABLE' && item.path === '/captureInput'))
  const malformed = '{"capture":'
  const degraded = readEvidence(operation, malformed)
  assert.equal(degraded.captureText, malformed)
  assert.equal(degraded.captureStatus, 'invalid')
  assert.equal(degraded.operation.result.decision, 'rejected')
  const largeOperation = JSON.stringify({ ...saved.report.operation, extra: 'x'.repeat(4 * 1024 * 1024) })
  assert.throws(() => readEvidence(largeOperation, ' '.repeat(4 * 1024 * 1024)), (error) => error.code === 'INPUT_LIMIT')
})

test('parsed negative zero is preserved as payload and rejected as unsigned metadata', () => {
  const saved = rejected()
  nested(saved).messages[0].payload = -0
  assert.equal(JSON.stringify(readEvidence(saved).source.messageSeries.messageSeries.messages[0].payload), '-0')
  saved.report.operation.latestEventId = -0
  assert.throws(() => readEvidence(saved), /decimal/)
})

test('cycles and cross-correlation records remain inspectable but never ordinary causal facts', () => {
  for (const mutate of [
    (s) => { nested(s).messages[1].causationId = nested(s).messages[1].messageId },
    (s) => { nested(s).messages[0].causationId = nested(s).messages[1].messageId },
    (s) => { nested(s).messages[1].correlationId = 'another-correlation' },
  ]) {
    const saved = accepted()
    mutate(saved)
    const view = readEvidence(saved)
    assert.equal(view.captureStatus, 'anomalous')
    assert.equal(view.report.status, 'passed')
    const eventId = nested(saved).messages[1].messageId
    assert.ok(!view.edges.some((edge) => edge.childMessageId === eventId || edge.parentMessageId === eventId))
    const selection = createSelection(view, eventId)
    assert.ok(resolveSelection(view, selection).diagnostics.length)
    assert.ok(!renderText(view, { selection }).includes('caused by:'))
  }
  const saved = rejected()
  nested(saved).commandOutcomes[0].correlationId = 'another-correlation'
  const view = readEvidence(saved)
  assert.ok(codes(view).includes('CROSS_CORRELATION'))
  assert.equal(resolveSelection(view, createSelection(view, firstId(view))).outcome, undefined)
})

test('valid child-before-parent and independently arriving outcomes remain valid', () => {
  const saved = accepted()
  nested(saved).messages.reverse()
  nested(saved).messages.forEach((message, index) => { message.observationOrder = index + 1 })
  nested(saved).commandOutcomes[0].observationOrder = 0
  const view = readEvidence(saved)
  assert.equal(view.captureStatus, 'valid')
  assert.equal(view.edges.length, 2)
})

test('input bytes/depth/work and text have explicit budgets', () => {
  assert.throws(() => readEvidence(' '.repeat(8 * 1024 * 1024 + 1)), (error) => error.code === 'INPUT_LIMIT')
  const deep = '{"value":' + '['.repeat(10000) + '0' + ']'.repeat(10000) + '}'
  assert.throws(() => readEvidence(deep), (error) => error.code === 'DEPTH_LIMIT')
  let object = 0
  for (let i = 0; i < 100; i++) object = [object]
  assert.throws(() => readEvidence({ object }), (error) => error.code === 'DEPTH_LIMIT')
  const saved = rejected()
  saved.messageSeries.capture.note = '<'.repeat(262144)
  nested(saved).messages[0].payload = { huge: '*'.repeat(262144) }
  const view = readEvidence(JSON.stringify(saved))
  for (const options of [{}, { includePayloads: true }, { maxCharacters: 1024, includePayloads: true }]) {
    const text = renderText(view, options)
    assert.ok(text.length <= (options.maxCharacters ?? 16000))
    assert.match(text, /omitted/i)
  }
  assert.throws(() => renderText(view, { maxCharacters: Infinity }), /budget/i)
})
