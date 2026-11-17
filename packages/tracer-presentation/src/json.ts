import type { Json, JsonObject, RawJsonNumber } from './types.ts'
import { EvidenceError } from './errors.ts'

// Node 22.16 supports both APIs; declaring their narrow shape also permits
// checking with the repository's TypeScript lib.es2023.
const nativeJson = JSON as typeof JSON & {
  rawJSON(source: string): RawJsonNumber
  isRawJSON(value: unknown): value is RawJsonNumber
}

export const MAX_INPUT_BYTES = 8 * 1024 * 1024
const MAX_DEPTH = 64
const MAX_NODES = 200000
export const byteLength = (value: string) => new TextEncoder().encode(value).length

function preflight(source: string) {
  if (source.length > MAX_INPUT_BYTES || byteLength(source) > MAX_INPUT_BYTES) throw new EvidenceError('INPUT_LIMIT', 'Evidence exceeds the 8 MiB input budget')
  let depth = 0
  let quoted = false
  let escaped = false
  for (const char of source) {
    if (quoted) {
      if (escaped) escaped = false
      else if (char === '\\') escaped = true
      else if (char === '"') quoted = false
    } else if (char === '"') quoted = true
    else if (char === '{' || char === '[') {
      if (++depth > MAX_DEPTH) throw new EvidenceError('DEPTH_LIMIT', 'Evidence exceeds the 64-level depth budget')
    } else if (char === '}' || char === ']') depth--
  }
}

export function readJson(input: unknown): JsonObject {
  if (typeof input === 'string') preflight(input)
  let nodes = 0
  const value = typeof input === 'string'
    ? JSON.parse(input, (_key: string, item: unknown, context?: { source: string }) => {
      if (++nodes > MAX_NODES) throw new EvidenceError('INPUT_LIMIT', 'Evidence exceeds the JSON value-count budget')
      // A Number is usable only when serialization reproduces its original token
      // exactly. This catches fractions rounding to safe integers and underflow,
      // as well as exponent spelling, negative zero and unsafe integers.
      if (typeof item === 'number') {
        if (!context) throw new Error('Lossless JSON parsing requires Node 22.16+')
        if (JSON.stringify(item) !== context.source || !Number.isFinite(item) || (Number.isInteger(item) && !Number.isSafeInteger(item))) {
          return nativeJson.rawJSON(context.source)
        }
      }
      return item
    })
    : input
  return object(copyJson(value, new Set(), 0, { nodes: 0, bytes: 0 }), 'evidence')
}

function copyJson(value: unknown, seen: Set<object>, depth: number, budget: { nodes: number; bytes: number }): Json {
  if (depth > MAX_DEPTH) throw new EvidenceError('DEPTH_LIMIT', 'Evidence exceeds the 64-level depth budget')
  if (++budget.nodes > MAX_NODES) throw new EvidenceError('INPUT_LIMIT', 'Evidence exceeds the JSON value-count budget')
  budget.bytes += typeof value === 'string' ? byteLength(value) : 8
  if (budget.bytes > MAX_INPUT_BYTES) throw new EvidenceError('INPUT_LIMIT', 'Evidence exceeds the input budget')
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return value
  if (typeof value === 'number') {
    if (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value))) {
      throw new Error('Unsafe numeric input; pass the original JSON text to preserve precision')
    }
    if (Object.is(value, -0)) return nativeJson.rawJSON('-0')
    return value
  }
  if (nativeJson.isRawJSON(value)) {
    if (!/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/.test(value.rawJSON)) {
      throw new EvidenceError('INVALID_JSON', 'Only numeric raw-JSON tokens are supported in evidence objects')
    }
    budget.bytes += value.rawJSON.length
    if (budget.bytes > MAX_INPUT_BYTES) throw new EvidenceError('INPUT_LIMIT', 'Evidence exceeds the input budget')
    return nativeJson.rawJSON(value.rawJSON)
  }
  if (typeof value !== 'object' || seen.has(value)) throw new Error('Evidence must contain only acyclic JSON data')
  if (!Array.isArray(value) && ![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
    throw new Error('Evidence must contain plain JSON objects')
  }
  seen.add(value)
  if (Array.isArray(value) && value.length > MAX_NODES) throw new EvidenceError('INPUT_LIMIT', 'Evidence exceeds the array-length budget')
  const entries = Object.entries(Object.getOwnPropertyDescriptors(value)).filter(([key]) => !(Array.isArray(value) && key === 'length'))
  if (Array.isArray(value) && (entries.length !== value.length || entries.some(([key], index) => key !== String(index)))) {
    throw new Error('Evidence arrays must be dense JSON arrays without extra properties')
  }
  for (const [, descriptor] of entries) {
    if (!('value' in descriptor)) throw new Error('Evidence must not contain accessors')
  }
  for (const [key] of entries) budget.bytes += byteLength(key)
  const result = Array.isArray(value)
    ? entries.map(([, descriptor]) => copyJson(descriptor.value, seen, depth + 1, budget))
    : Object.fromEntries(entries.map(([key, descriptor]) => [key, copyJson(descriptor.value, seen, depth + 1, budget)]))
  seen.delete(value)
  return result
}

export function object(value: unknown, field: string): JsonObject {
  if (value === null || typeof value !== 'object' || Array.isArray(value) || nativeJson.isRawJSON(value)) {
    throw new Error(`Invalid ${field}: expected an object`)
  }
  return value as JsonObject
}

export function decimal(value: unknown, field: string): string {
  const raw = nativeJson.isRawJSON(value) ? value.rawJSON : value
  const text = typeof raw === 'number' && Number.isSafeInteger(raw) ? String(raw) : raw
  if (typeof text !== 'string' || text.length > 20 || !/^(0|[1-9][0-9]*)$/.test(text) || BigInt(text) > 18446744073709551615n) {
    throw new Error(`Invalid ${field}: expected an exact unsigned 64-bit decimal`)
  }
  return text
}

export function freeze<T>(value: T): T {
  if (value && typeof value === 'object' && !Object.isFrozen(value)) {
    for (const item of Object.values(value)) freeze(item)
    Object.freeze(value)
  }
  return value
}

// All user-controlled text stays quoted and on one line. Escape markup and
// Unicode format/control characters as data, without changing stored identities.
export function quote(value: string, maximum = 1024): string {
  const omitted = value.length > maximum ? ` [omitted ${value.length - maximum} string characters]` : ''
  return JSON.stringify(value.slice(0, maximum)).replace(/[<>&`\[\]*#\p{Cf}\u007f-\u009f\u2028\u2029]|(?<![A-Za-z0-9])_|_(?![A-Za-z0-9])/gu,
    (char) => char.split('').map((part) => `\\u${part.charCodeAt(0).toString(16).padStart(4, '0')}`).join('')) + omitted
}

function* jsonTokens(value: Json): Generator<string> {
  if (nativeJson.isRawJSON(value)) { yield value.rawJSON; return }
  if (typeof value === 'string') { yield quote(value); return }
  if (Array.isArray(value)) {
    yield '['
    for (let i = 0; i < value.length; i++) { if (i) yield ','; yield* jsonTokens(value[i]) }
    yield ']'
  } else if (value !== null && typeof value === 'object') {
    yield '{'
    let first = true
    for (const key of Object.keys(value).sort()) {
      if (!first) yield ','
      first = false
      yield quote(key)
      yield ':'
      yield* jsonTokens((value as JsonObject)[key])
    }
    yield '}'
  } else yield JSON.stringify(value)
}

export function jsonText(value: Json, maximum = 4096): string {
  let result = ''
  for (const token of jsonTokens(value)) {
    if (result.length + token.length > maximum) return result.slice(0, maximum - 32) + ' [omitted remaining JSON preview]'
    result += token
  }
  return result
}

/** Exact deterministic comparison, separate from bounded display previews. */
export function comparableJson(value: Json): string {
  if (nativeJson.isRawJSON(value)) return value.rawJSON
  if (typeof value === 'string') return JSON.stringify(value)
  if (Array.isArray(value)) return `[${value.map(comparableJson).join(',')}]`
  if (value !== null && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${comparableJson((value as JsonObject)[key])}`).join(',')}}`
  }
  return JSON.stringify(value)
}
