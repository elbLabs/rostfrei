import { describe, expect, expectTypeOf, it } from 'vitest'

import { parseDomainModel } from './schema'
import type {
  CanonicalScalarType,
  ScalarType,
  SemanticScalarType,
  ValueObject,
  ValueObjectVariantShape,
} from './schema'

function emptyModel(): Record<string, unknown> {
  return {
    boundedContexts: [],
    aggregates: [],
    entities: [],
    domainIdentities: [],
    valueObjects: [],
    domainServices: [],
    domainCommands: [],
    domainEvents: [],
    domainErrors: [],
    actions: [],
    decisions: [],
    queries: [],
    invariants: [],
  }
}

describe('parseDomainModel', () => {
  it('returns a typed model with decisions and all required collections', () => {
    const input = emptyModel()
    const aggregate = { context: 'sales', local: 'orders' }
    const request = { owner: { kind: 'aggregate', id: aggregate }, local: 'decisionRequest' }
    const result = { owner: { kind: 'aggregate', id: aggregate }, local: 'decisionResult' }
    const decision = {
      id: { owner: { kind: 'aggregate', id: aggregate }, local: 'decide' },
      label: 'Decide',
      input: { kind: 'valueObject', id: request },
      output: { kind: 'valueObject', id: result },
      implementation: { kind: 'rust' },
    }
    input.boundedContexts = [{ id: 'sales', label: 'Sales' }]
    input.decisions = [decision]

    const model = parseDomainModel(input)

    expect(model).toBe(input)
    expect(model.boundedContexts[0]?.label).toBe('Sales')
    expect(model.decisions[0]).toEqual(decision)
    expect(model.decisions[0]?.id.owner.kind).toBe('aggregate')
    expect(model.decisions[0]?.input.id).toEqual(request)
    expect(model.decisions[0]?.output.id).toEqual(result)
    expect(model.decisions[0]?.implementation.kind).toBe('rust')
  })

  it('types and preserves canonical and semantic scalar JSON for fields and identities', () => {
    const input = emptyModel()
    const aggregate = { context: 'sales', local: 'orders' }
    const entity = { aggregate, local: 'order' }
    const canonical: CanonicalScalarType = 'string'
    const semantic: SemanticScalarType = {
      kind: 'semantic',
      id: 'uuid',
      label: 'UUID',
      representation: canonical,
    }
    input.domainIdentities = [{ id: { owner: entity }, scalar: semantic }]
    input.valueObjects = [{
      id: { owner: { kind: 'entity', id: entity }, local: 'external-id' },
      label: 'External ID',
      fields: [
        { name: 'canonical', value: { kind: 'scalar', scalar: canonical } },
        { name: 'semantic', value: { kind: 'scalar', scalar: semantic } },
      ],
    }]

    const model = parseDomainModel(JSON.parse(JSON.stringify(input)))
    const identityScalar = model.domainIdentities[0]!.scalar
    const fields = model.valueObjects[0]!.fields!

    expectTypeOf(identityScalar).toEqualTypeOf<ScalarType>()
    expect(fields[0]!.value).toEqual({ kind: 'scalar', scalar: 'string' })
    expect(fields[1]!.value).toEqual({ kind: 'scalar', scalar: semantic })
    expect(identityScalar).toEqual(semantic)
    if (typeof identityScalar !== 'string') {
      expectTypeOf(identityScalar).toEqualTypeOf<SemanticScalarType>()
      expect(identityScalar.representation).toBe('string')
    }
  })

  it('types and preserves struct, legacy enum, and tagged enum Value Objects', () => {
    const input = emptyModel()
    const owner = { kind: 'boundedContext', id: 'sales' }
    input.valueObjects = [
      { id: { owner, local: 'amount' }, label: 'Amount', fields: [] },
      { id: { owner, local: 'state' }, label: 'State', variants: ['Open', 'Closed'] },
      {
        id: { owner, local: 'change' },
        label: 'Change',
        variants: ['None', 'Retry', 'Moved'],
        variantShapes: [
          { name: 'None', kind: 'unit' },
          { name: 'Retry', kind: 'tuple', fields: [] },
          { name: 'Moved', kind: 'struct', fields: [] },
        ],
      },
    ]

    const model = parseDomainModel(JSON.parse(JSON.stringify(input)))

    expectTypeOf(model.valueObjects).toEqualTypeOf<ValueObject[]>()
    expect(model.valueObjects).toEqual(input.valueObjects)
    expect(model.valueObjects[0]).toMatchObject({ fields: [] })
    expect(model.valueObjects[1]).toMatchObject({ variants: ['Open', 'Closed'] })
    const tagged = model.valueObjects[2]!
    expect(tagged).toMatchObject({
      variants: ['None', 'Retry', 'Moved'],
      variantShapes: [
        { name: 'None', kind: 'unit' },
        { name: 'Retry', kind: 'tuple', fields: [] },
        { name: 'Moved', kind: 'struct', fields: [] },
      ],
    })
    if (tagged.variantShapes !== undefined) {
      expectTypeOf(tagged.variantShapes).toEqualTypeOf<ValueObjectVariantShape[]>()
      expect(tagged.variantShapes[0]).not.toHaveProperty('fields')
      expect(tagged.variantShapes[1]!.fields).toEqual([])
      expect(tagged.variantShapes[2]!.fields).toEqual([])
    }
  })

  it('keeps lifecycle optional for entities without lifecycle metadata', () => {
    const input = emptyModel()
    const aggregate = { context: 'sales', local: 'orders' }
    const entity = { aggregate, local: 'order' }
    input.entities = [{
      id: entity,
      label: 'Order',
      identity: { field: 'id', id: { owner: entity } },
      fields: [],
    }]

    const model = parseDomainModel(input)

    expect(model.entities[0]?.lifecycle).toBeUndefined()
    expect(model.entities[0]).not.toHaveProperty('lifecycle')
  })

  it.each([null, [], 'model', 1])('rejects non-object input: %j', (input) => {
    expect(() => parseDomainModel(input)).toThrow('Domain model must be an object')
  })

  it('rejects every missing or non-array top-level collection', () => {
    for (const collection of Object.keys(emptyModel())) {
      const model = emptyModel()
      model[collection] = {}
      expect(() => parseDomainModel(model)).toThrow(
        `Domain model ${collection} must be an array`,
      )
    }
  })
})
