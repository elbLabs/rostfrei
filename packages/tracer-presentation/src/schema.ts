import { Ajv2020, type ValidateFunction } from 'ajv/dist/2020.js'
import observedSchema from './schema/observed-message-series-v2.schema.json' with { type: 'json' }
import behavioralSchema from './schema/behavioral-test-v2.schema.json' with { type: 'json' }
import { EvidenceError } from './errors.ts'
import { byteLength, object } from './json.ts'
import type { Json, JsonObject } from './types.ts'

// These are byte-identical Rust-generated contracts, checked by conformance
// tests. Disable format annotations only (uint32/64 are bounded by schema).
const ajv = new Ajv2020({ strict: false, validateFormats: false, allErrors: false })
const observed = ajv.compile(observedSchema)
const outcome = ajv.compile({ $defs: observedSchema.$defs, $ref: '#/$defs/ObservedCommandOutcome' })
const timeout = ajv.compile(behavioralSchema.$defs.BehavioralTimeoutSchema)

function validate(validator: ValidateFunction, value: unknown, path: string) {
  if (!validator(value)) {
    // oneOf includes failures from nonmatching alternatives. The deepest source
    // pointer is more useful than the first alternative's discriminator failure.
    const issue = validator.errors?.reduce((best, item) => item.instancePath.length > best.instancePath.length ? item : best)
    const property = issue?.keyword === 'additionalProperties' ? String(issue.params.additionalProperty).replaceAll('~', '~0').replaceAll('/', '~1') : undefined
    throw new EvidenceError('INVALID_CAPTURE', `Canonical schema violation: ${issue?.keyword ?? 'unsupported shape'}`, path + (issue?.instancePath ?? '') + (property ? `/${property}` : ''))
  }
}

export function checkObserved(value: Json, path: string) {
  validate(observed, value, path)
  const series = value as JsonObject
  const messages = series.messages as readonly JsonObject[]
  const outcomes = series.commandOutcomes as readonly JsonObject[]
  const orders = new Set<number>()
  const ids = new Set<Json>()
  const commands = new Set<Json>()
  const responses = new Set<Json>()
  for (const [section, records] of [['messages', messages], ['commandOutcomes', outcomes]] as const) {
    let previous = -1
    for (const [index, record] of records.entries()) {
      const location = `${path}/${section}/${index}`
      const order = record.observationOrder as number
      if (!Number.isSafeInteger(order) || order < 0 || order >= messages.length + outcomes.length || order <= previous || orders.has(order)) {
        throw new EvidenceError('INVALID_CAPTURE', 'Invalid observation order: expected unique contiguous insertion ordinals', `${location}/observationOrder`)
      }
      previous = order
      orders.add(order)
      if (section === 'messages') {
        if (ids.has(record.messageId)) throw new EvidenceError('INVALID_CAPTURE', 'Duplicate message identity', `${location}/messageId`)
        ids.add(record.messageId)
      } else {
        if (commands.has(record.commandMessageId) || responses.has(record.responseMessageId)) throw new EvidenceError('INVALID_CAPTURE', 'Duplicate outcome identity', location)
        commands.add(record.commandMessageId)
        responses.add(record.responseMessageId)
        checkOutcome(record, location)
      }
    }
  }
}

export function checkOutcome(value: Json, path: string) {
  validate(outcome, value, path)
  const result = object(object(value, path).outcome, `${path}/outcome`)
  if (result.status === 'rejected') {
    const rejection = object(result.value, `${path}/outcome/value`)
    const message = rejection.message as string
    // Rust CommandRejection::new supplements the generated JSON Schema.
    if (message.length === 0 || /\p{Cc}/u.test(message) || byteLength(message) > 1024) {
      throw new EvidenceError('INVALID_CAPTURE', 'Rejection message violates the runtime text contract', `${path}/outcome/value/message`)
    }
  }
}

export function checkTimeout(value: Json, path: string) { validate(timeout, value, path) }
