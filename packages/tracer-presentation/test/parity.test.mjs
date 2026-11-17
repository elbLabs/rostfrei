import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { readEvidence, createSelection, resolveSelection } from '../src/index.ts'

// The Rust integration test consumes this exact corpus. It is the oracle for
// serialized-series validity and diagnostics, not for presentation-only policy.
const corpus = JSON.parse(readFileSync(new URL('../../../crates/rostfrei-tracer/tests/fixtures/presentation-contract-v2.json', import.meta.url), 'utf8'))
const native = JSON.parse(readFileSync(new URL('./canonical/accepted.json', import.meta.url), 'utf8'))
assert.equal(corpus.formatVersion, 1)
assert.equal(corpus.contract, 'observed-message-series-v2')
assert.equal(new Set([...corpus.cases, ...corpus.limits].map((item) => item.name)).size, corpus.cases.length + corpus.limits.length)

function member(document, path) {
  assert.ok(path.startsWith('/'))
  const keys = path.slice(1).split('/').map((key) => key.replaceAll('~1', '/').replaceAll('~0', '~'))
  const key = keys.pop()
  const object = keys.reduce((value, part) => {
    assert.ok(Object.hasOwn(value, part), `Missing fixture parent: ${path}`)
    return value[part]
  }, document)
  assert.ok(object !== null && typeof object === 'object' && !Array.isArray(object), path)
  return { object, key }
}

function set(document, path, value) {
  const { object, key } = member(document, path)
  Object.defineProperty(object, key, { value: structuredClone(value), enumerable: true, writable: true, configurable: true })
}

function expand(item) {
  assert.ok(Object.hasOwn(corpus.bases, item.base))
  const document = structuredClone(corpus.bases[item.base])
  if (item.kind === 'array') {
    assert.ok(['messages', 'commandOutcomes'].includes(item.field))
    const template = document[item.field][0]
    document.messages = []
    document.commandOutcomes = []
    document[item.field] = Array.from({ length: item.count }, (_, index) => ({
      ...structuredClone(template), observationOrder: index,
      ...(item.field === 'messages'
        ? { messageId: `m-${index}` }
        : { commandMessageId: `c-${index}`, responseMessageId: `r-${index}` }),
    }))
  } else if (item.kind === 'string') {
    set(document, item.path, item.unit.repeat(item.count))
  } else {
    assert.equal(item.kind, undefined)
    for (const [path, value] of Object.entries(item.set ?? {})) set(document, path, value)
    for (const path of item.remove ?? []) {
      const { object, key } = member(document, path)
      assert.ok(Object.hasOwn(object, key), `Missing fixture member: ${path}`)
      delete object[key]
    }
  }
  return document
}

function present(messageSeries) {
  // Reuse actual canonical envelopes. A running snapshot without a result keeps
  // independent wire cases free of invented cross-source business agreements.
  const operation = structuredClone(native.report.operation)
  operation.status = 'running'
  delete operation.result
  operation.correlationId = corpus.bases.accepted.messages[0].correlationId
  operation.correlationEventsHref = `/correlations/${operation.correlationId}/events`
  operation.events.href = operation.correlationEventsHref
  const capture = structuredClone(native.messageSeries)
  capture.correlationId = operation.correlationId
  capture.capture.settled = false
  capture.messageSeries = messageSeries
  return readEvidence(JSON.stringify(operation), JSON.stringify(capture))
}

const base = '/captureInput/messageSeries'
const ordered = (values) => values.map((value) => JSON.stringify(value)).sort()

function assertStructural(view, expected) {
  assert.equal(view.kind, 'operation')
  assert.equal(view.operation.status, 'running')
  if (!expected) {
    assert.equal(view.captureStatus, 'invalid')
    assert.equal(view.series, undefined)
    assert.deepEqual(view.edges, [])
    assert.ok(view.diagnostics.some((diagnostic) => diagnostic.code === 'INVALID_CAPTURE' && diagnostic.path.startsWith(base)))
    assert.throws(() => createSelection(view, corpus.bases.accepted.messages[0].messageId), { code: 'CAPTURE_UNAVAILABLE' })
    return false
  }
  assert.ok(['valid', 'anomalous'].includes(view.captureStatus))
  assert.ok(view.series)
  assert.ok(!view.diagnostics.some((diagnostic) => diagnostic.code === 'INVALID_CAPTURE'))
  return true
}

function assertFacts(view, expected) {
  const { messages, commandOutcomes } = view.series.messageSeries
  const byId = new Map(messages.map((message, index) => [message.messageId, { message, index }]))
  const messagePath = (id) => `${base}/messages/${byId.get(id).index}`
  const outcomePath = (response) => `${base}/commandOutcomes/${commandOutcomes.findIndex((outcome) => outcome.responseMessageId === response)}`
  const matches = messages.filter((child) => byId.has(child.causationId)).map((child) => [child.causationId, child.messageId])
  assert.deepEqual(matches, expected.causationMatches)

  const associations = commandOutcomes.flatMap((outcome) => {
    const command = byId.get(outcome.commandMessageId)?.message
    if (!command) return []
    const resolved = resolveSelection(view, createSelection(view, command.messageId))
    return resolved.outcome ? [[command.messageId, resolved.outcome.responseMessageId]] : []
  })
  assert.deepEqual(associations, expected.outcomeAssociations)

  // Map Rust issues into the public presentation diagnostic vocabulary. Missing
  // parent is a warning; other canonical issues are errors. notACommand and
  // missingCommand intentionally share MISSING_COMMAND in the presentation API.
  const diagnostics = []
  const anomalous = new Set()
  const unassociated = []
  const add = (code, severity, path, messageIds, responseMessageIds = []) => {
    diagnostics.push({ code, severity, path, messageIds: [...messageIds].sort(), responseMessageIds: [...responseMessageIds].sort() })
  }
  for (const issue of expected.topologyIssues) {
    if (issue.kind === 'missingParent') {
      add('MISSING_PARENT', 'warning', `${messagePath(issue.child)}/causationId`, [issue.child])
    } else if (issue.kind === 'crossCorrelation') {
      add('CROSS_CORRELATION', 'error', `${messagePath(issue.child)}/causationId`, [issue.child])
      anomalous.add(issue.child)
      const diagnostic = view.diagnostics.find((item) => item.code === 'CROSS_CORRELATION' && item.path === `${messagePath(issue.child)}/causationId`)
      assert.ok(diagnostic?.relatedPaths?.includes(messagePath(issue.parent)))
    } else {
      assert.equal(issue.kind, 'cycle')
      issue.messages.forEach((id) => anomalous.add(id))
      const diagnostic = view.diagnostics.find((item) => item.code === 'CAUSATION_CYCLE' && ordered(item.messageIds ?? []).join() === ordered(issue.messages).join())
      assert.ok(diagnostic)
      assert.ok(issue.messages.some((id) => diagnostic.path === messagePath(id)))
      assert.deepEqual(ordered(diagnostic.relatedPaths), ordered(issue.messages.map(messagePath)))
      // Rust normalizes cycle rotation; presentation may start at another member.
      add('CAUSATION_CYCLE', 'error', diagnostic.path, issue.messages)
    }
  }
  for (const issue of expected.outcomeIssues) {
    const cross = issue.kind === 'crossCorrelation'
    assert.ok(cross || ['missingCommand', 'notACommand'].includes(issue.kind))
    add(cross ? 'CROSS_CORRELATION' : 'MISSING_COMMAND', 'error', outcomePath(issue.response) + (cross ? '/correlationId' : ''), [issue.command], [issue.response])
    unassociated.push(issue.response)
  }
  // An operation envelope adds a constraint absent from bare Rust MessageSeries:
  // a mismatched message also receives an envelope-correlation diagnostic.
  for (const message of messages) {
    if (message.correlationId !== view.operation.correlationId) {
      anomalous.add(message.messageId)
      add('CROSS_CORRELATION', 'error', `${messagePath(message.messageId)}/correlationId`, [message.messageId])
    }
  }
  const actual = view.diagnostics.filter((item) => item.path.startsWith(base)).map((item) => ({
    code: item.code, severity: item.severity, path: item.path,
    messageIds: [...(item.messageIds ?? [])].sort(), responseMessageIds: [...(item.responseMessageIds ?? [])].sort(),
  }))
  assert.deepEqual(ordered(actual), ordered(diagnostics))
  assert.deepEqual([...view.anomalousMessageIds].sort(), [...anomalous].sort())
  assert.deepEqual([...view.unassociatedOutcomeIds].sort(), unassociated.sort())
  assert.equal(view.captureStatus, diagnostics.some((item) => item.severity === 'error') ? 'anomalous' : 'valid')

  // Presentation withholds every edge incident to an anomalous message, including
  // an otherwise noncyclic tail attached to a cycle. Raw matches remain retained.
  const safeEdges = expected.causationMatches.filter(([parent, child]) => !anomalous.has(parent) && !anomalous.has(child))
  assert.deepEqual(view.edges.map((edge) => [edge.parentMessageId, edge.childMessageId]), safeEdges)
}

for (const item of corpus.cases) {
  test(`Rust parity: ${item.name}`, () => {
    const series = expand(item)
    const view = present(series)
    if (!assertStructural(view, item.expect.structurallyValid)) return
    assert.deepEqual(view.series.messageSeries, series)
    assertFacts(view, item.expect)
  })
}

for (const item of corpus.limits) {
  test(`Rust parity bound: ${item.name}`, () => {
    const series = expand(item)
    const view = present(series)
    // The shared bound recipes assert structural validity only; outcome-only
    // arrays and changed command IDs can legitimately be anomalous, not invalid.
    if (assertStructural(view, item.structurallyValid)) assert.deepEqual(view.series.messageSeries, series)
  })
}
