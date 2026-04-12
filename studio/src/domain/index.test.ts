import { describe, expect, it } from 'vitest'

import type { DomainModel } from './schema'
import {
  aggregateKey,
  buildDomainIndex,
  contextKey,
  entityKey,
  getBreadcrumbTrail,
  identityKey,
  serviceKey,
  valueObjectKey,
} from './index'

const sales = { context: 'sales', local: 'orders' }
const order = { aggregate: sales, local: 'order' }
const line = { aggregate: sales, local: 'line' }
const orderIdentity = { owner: order }
const money = { owner: { kind: 'boundedContext' as const, id: 'sales' }, local: 'money' }
const status = { owner: { kind: 'aggregate' as const, id: sales }, local: 'status' }
const note = { owner: { kind: 'entity' as const, id: line }, local: 'note' }
const checkout = { context: 'sales', local: 'checkout' }
const uuidScalar = { kind: 'semantic' as const, id: 'uuid', label: 'UUID', representation: 'string' as const }

function model(): DomainModel {
  return {
    boundedContexts: [{ id: 'sales', label: 'Sales' }],
    aggregates: [{ id: sales, label: 'Orders', root: order }],
    entities: [
      { id: line, label: 'Order Line', identity: { field: 'id', id: { owner: line } }, fields: [] },
      { id: order, label: 'Order', identity: { field: 'id', id: orderIdentity }, fields: [
        { name: 'id', value: { kind: 'identity', id: orderIdentity } },
        { name: 'lines', value: { kind: 'list', element: { kind: 'entity', id: line } } },
        { name: 'status', value: { kind: 'optional', value: { kind: 'valueObject', id: status } } },
      ] },
    ],
    domainIdentities: [
      { id: orderIdentity, scalar: 'u64' },
      { id: { owner: line }, scalar: 'u64' },
    ],
    valueObjects: [
      { id: money, label: 'Money', fields: [{ name: 'amount', value: { kind: 'scalar', scalar: 'i64' } }] },
      { id: status, label: 'Order Status', variants: ['Draft', 'Placed'] },
      { id: note, label: 'Line Note', fields: [{ name: 'text', value: { kind: 'scalar', scalar: 'string' } }] },
    ],
    domainServices: [{ id: checkout, label: 'Checkout' }],
    domainCommands: [{ id: { owner: { kind: 'aggregate', id: sales }, local: 'place' }, label: 'Place Order', fields: [] }],
    domainEvents: [{ id: { aggregate: sales, local: 'placed' }, label: 'Order Placed', fields: [] }],
    domainErrors: [{ id: { owner: { kind: 'aggregate', id: sales }, local: 'rejected' }, label: 'Order Rejected', code: 'REJECTED', message: 'Rejected', fields: [] }],
    actions: [
      { id: { owner: { kind: 'aggregate', id: sales }, local: 'place' }, label: 'Place',
        input: { kind: 'domainCommand', id: { owner: { kind: 'aggregate', id: sales }, local: 'place' } },
        output: { kind: 'domainEvent', id: { aggregate: sales, local: 'placed' } },
        error: { owner: { kind: 'aggregate', id: sales }, local: 'rejected' } },
      { id: { owner: { kind: 'entity', id: line }, local: 'clear' }, label: 'Clear', input: null, output: null, error: null },
    ],
    decisions: [],
    queries: [{ id: { aggregate: sales, local: 'find' }, label: 'Find Order',
      input: { kind: 'domainIdentity', id: orderIdentity },
      output: { kind: 'optional', value: { kind: 'valueObject', id: status } } }],
    invariants: [{ id: { owner: { kind: 'aggregate', id: sales }, local: 'hasLines' }, label: 'Has lines' }],
  }
}

function modelWithLifecycle(): DomainModel {
  const result = model()
  const restoreAction = { owner: { kind: 'entity' as const, id: line }, local: 'restore' }
  result.actions.push({
    id: restoreAction,
    label: 'Restore',
    input: null,
    output: null,
    error: null,
  })
  result.entities[0]!.lifecycle = {
    id: 'lineLifecycle',
    label: 'Order line lifecycle',
    states: [
      { id: 'active', label: 'Active' },
      { id: 'cleared', label: 'Cleared' },
    ],
    initial: 'active',
    transitions: [
      { source: 'active', action: result.actions[1]!.id, target: 'cleared' },
      { source: 'cleared', action: restoreAction, target: 'active' },
    ],
  }
  return result
}

describe('buildDomainIndex', () => {
  it('builds ordered navigation, parents, and breadcrumbs', () => {
    const index = buildDomainIndex(model())
    const context = index.sidebar[0]!
    expect(context.children.map((item) => item.label)).toEqual(['Orders', 'Money', 'Checkout'])
    expect(context.children[0]!.children.map((item) => [item.label, item.root])).toEqual([
      ['Order', true], ['Order Line', false], ['Order Status', false],
    ])
    expect(context.children[0]!.children[1]!.children.map((item) => item.label)).toEqual(['Line Note'])
    expect(index.initialSelection).toBe(aggregateKey(sales))
    expect(getBreadcrumbTrail(index, valueObjectKey(note)).map((item) => item.label)).toEqual([
      'Sales', 'Orders', 'Order Line', 'Line Note',
    ])
  })

  it('groups behavior and resolves linked recursive types and top-level labels', () => {
    const index = buildDomainIndex(model())
    const aggregate = index.selections.get(aggregateKey(sales))!
    expect(aggregate.behavior.actions[0]).toMatchObject({ visibility: 'Public', input: { name: 'Place Order' }, output: { name: 'Order Placed' }, error: { name: 'Order Rejected' } })
    expect(aggregate.behavior.queries[0]!.output).toEqual({ kind: 'optional', value: { kind: 'reference', name: 'Order Status', key: valueObjectKey(status) } })
    expect(aggregate.behavior.invariants[0]!.label).toBe('Has lines')
    expect(index.selections.get(entityKey(line))!.behavior.actions[0]).toMatchObject({ visibility: 'Internal', input: null, output: { kind: 'unit', name: '()' } })

    const root = index.selections.get(entityKey(order))!
    expect(root.data).toMatchObject({ kind: 'struct', fields: [
      { type: { name: 'Identity of Order' } },
      { type: { kind: 'list', element: { name: 'Order Line' } } },
      { type: { kind: 'optional', value: { name: 'Order Status' } } },
    ] })
    expect(root.rustName).toBeUndefined()
  })

  it('normalizes legacy fieldless enum variants to unit presentation variants', () => {
    const selection = buildDomainIndex(model()).selections.get(valueObjectKey(status))!

    expect(selection.data).toEqual({
      kind: 'enum',
      variants: [
        { name: 'Draft', shape: 'unit', fields: [] },
        { name: 'Placed', shape: 'unit', fields: [] },
      ],
    })
  })

  it('normalizes mixed tagged shapes through the shared field resolver in source order', () => {
    const source = model()
    source.valueObjects[1] = {
      id: status,
      label: 'Order Status',
      variants: ['Idle', 'Retry', 'Changed'],
      variantShapes: [
        { name: 'Idle', kind: 'unit' },
        { name: 'Retry', kind: 'tuple', fields: [
          { name: '0', value: { kind: 'scalar', scalar: 'bool' } },
          { name: '1', value: { kind: 'scalar', scalar: uuidScalar } },
          { name: '2', value: { kind: 'identity', id: orderIdentity } },
          { name: '3', value: { kind: 'entity', id: line } },
          { name: '4', value: { kind: 'valueObject', id: note } },
          { name: '5', value: { kind: 'aggregateReference', aggregate: sales } },
          { name: '6', value: { kind: 'list', element: { kind: 'optional', value: {
            kind: 'valueObject', id: status,
          } } } },
        ] },
        { name: 'Changed', kind: 'struct', fields: [
          { name: 'amount', value: { kind: 'valueObject', id: money } },
        ] },
      ],
    }

    const index = buildDomainIndex(source)
    const selection = index.selections.get(valueObjectKey(status))!

    expect(selection.data).toEqual({
      kind: 'enum',
      variants: [
        { name: 'Idle', shape: 'unit', fields: [] },
        { name: 'Retry', shape: 'tuple', fields: [
          { name: '0', type: { kind: 'scalar', name: 'bool' } },
          { name: '1', type: { kind: 'semanticScalar', id: 'uuid', name: 'UUID', representation: 'string' } },
          { name: '2', type: { kind: 'reference', name: 'Identity of Order', key: identityKey(orderIdentity) } },
          { name: '3', type: { kind: 'reference', name: 'Order Line', key: entityKey(line) } },
          { name: '4', type: { kind: 'reference', name: 'Line Note', key: valueObjectKey(note) } },
          { name: '5', type: { kind: 'reference', name: 'Orders', key: aggregateKey(sales) } },
          { name: '6', type: { kind: 'list', element: { kind: 'optional', value: {
            kind: 'reference', name: 'Order Status', key: valueObjectKey(status),
          } } } },
        ] },
        { name: 'Changed', shape: 'struct', fields: [
          { name: 'amount', type: { kind: 'reference', name: 'Money', key: valueObjectKey(money) } },
        ] },
      ],
    })
    expect(index.selections.get(valueObjectKey(money))!.data).toEqual({
      kind: 'struct',
      fields: [{ name: 'amount', type: { kind: 'scalar', name: 'i64' } }],
    })
  })

  it('preserves empty tuple and struct tagged shapes', () => {
    const source = model()
    source.valueObjects[1] = {
      id: status,
      label: 'Order Status',
      variants: ['Unit', 'Tuple', 'Struct'],
      variantShapes: [
        { name: 'Unit', kind: 'unit' },
        { name: 'Tuple', kind: 'tuple', fields: [] },
        { name: 'Struct', kind: 'struct', fields: [] },
      ],
    }

    expect(buildDomainIndex(source).selections.get(valueObjectKey(status))!.data).toEqual({
      kind: 'enum',
      variants: [
        { name: 'Unit', shape: 'unit', fields: [] },
        { name: 'Tuple', shape: 'tuple', fields: [] },
        { name: 'Struct', shape: 'struct', fields: [] },
      ],
    })
  })

  it('rejects tagged variant shape count and name alignment mismatches', () => {
    const wrongLength = model()
    wrongLength.valueObjects[1] = {
      id: status,
      label: 'Order Status',
      variants: ['Draft', 'Placed'],
      variantShapes: [{ name: 'Draft', kind: 'unit' }],
    }
    expect(() => buildDomainIndex(wrongLength)).toThrow(
      'Variant shape count mismatch for Order Status: expected 2, received 1',
    )

    const wrongName = model()
    wrongName.valueObjects[1] = {
      id: status,
      label: 'Order Status',
      variants: ['Draft', 'Placed'],
      variantShapes: [
        { name: 'Placed', kind: 'unit' },
        { name: 'Draft', kind: 'unit' },
      ],
    }
    expect(() => buildDomainIndex(wrongName)).toThrow(
      'Variant shape alignment mismatch for Order Status at index 0: expected Draft, received Placed',
    )
  })

  it('normalizes semantic scalar fields while preserving canonical scalar presentation', () => {
    const source = model()
    source.valueObjects[0]!.fields!.push({
      name: 'externalId',
      value: { kind: 'scalar', scalar: uuidScalar },
    })

    const selection = buildDomainIndex(source).selections.get(valueObjectKey(money))!

    expect(selection.data).toEqual({
      kind: 'struct',
      fields: [
        { name: 'amount', type: { kind: 'scalar', name: 'i64' } },
        { name: 'externalId', type: {
          kind: 'semanticScalar', id: 'uuid', name: 'UUID', representation: 'string',
        } },
      ],
    })
  })

  it('normalizes semantic scalars through nested field wrappers', () => {
    const source = model()
    source.entities[0]!.fields.push({
      name: 'externalIds',
      value: { kind: 'list', element: { kind: 'optional', value: {
        kind: 'scalar', scalar: uuidScalar,
      } } },
    })

    const selection = buildDomainIndex(source).selections.get(entityKey(line))!

    expect(selection.data).toEqual({
      kind: 'struct',
      fields: [{
        name: 'externalIds',
        type: { kind: 'list', element: { kind: 'optional', value: {
          kind: 'semanticScalar', id: 'uuid', name: 'UUID', representation: 'string',
        } } },
      }],
    })
  })

  it('normalizes semantic scalar identity representations', () => {
    const source = model()
    source.domainIdentities[0]!.scalar = uuidScalar

    const selection = buildDomainIndex(source).selections.get(identityKey(orderIdentity))!

    expect(selection.data).toEqual({
      kind: 'struct',
      fields: [{ name: 'value', type: {
        kind: 'semanticScalar', id: 'uuid', name: 'UUID', representation: 'string',
      } }],
    })
  })

  it('rejects semantic scalars at raw action boundaries', () => {
    const source = model()
    source.actions[0]!.input = { kind: 'scalar', scalar: uuidScalar } as never

    expect(() => buildDomainIndex(source)).toThrow(
      'Invalid action scalar: semantic scalars require a modeled field or Domain Identity',
    )
  })

  it('rejects semantic scalars at raw query boundaries, including nested outputs', () => {
    const source = model()
    source.queries[0]!.output = {
      kind: 'optional',
      value: { kind: 'scalar', scalar: uuidScalar },
    } as never

    expect(() => buildDomainIndex(source)).toThrow(
      'Invalid query scalar: semantic scalars require a modeled field or Domain Identity',
    )
  })

  it('projects complete referenced outcome metadata without flattening nested event output types', () => {
    const source = model()
    source.domainEvents[0]!.fields = [
      { name: 'orderId', value: { kind: 'identity', id: orderIdentity } },
      { name: 'statusHistory', value: { kind: 'list', element: {
        kind: 'optional', value: { kind: 'valueObject', id: status },
      } } },
    ]
    source.domainErrors[0]!.fields = [
      { name: 'reason', value: { kind: 'scalar', scalar: 'string' } },
      { name: 'lastStatus', value: { kind: 'optional', value: { kind: 'valueObject', id: status } } },
    ]
    source.actions[0]!.output = {
      kind: 'optional',
      value: { kind: 'list', element: { kind: 'domainEvent', id: source.domainEvents[0]!.id } },
    }

    const aggregate = buildDomainIndex(source).selections.get(aggregateKey(sales))!
    const action = aggregate.behavior.actions[0]!
    const eventKey = JSON.stringify(['domainEvent', aggregateKey(sales), 'placed'])
    const errorKey = JSON.stringify(['domainError', aggregateKey(sales), 'rejected'])

    expect(action.output).toEqual({
      kind: 'optional',
      value: { kind: 'list', element: { kind: 'reference', name: 'Order Placed' } },
    })
    expect(aggregate.behavior.domainEvents).toEqual([{
      key: eventKey,
      stableId: 'placed',
      label: 'Order Placed',
      fields: [
        { name: 'orderId', type: { kind: 'reference', name: 'Identity of Order', key: identityKey(orderIdentity) } },
        { name: 'statusHistory', type: { kind: 'list', element: {
          kind: 'optional', value: { kind: 'reference', name: 'Order Status', key: valueObjectKey(status) },
        } } },
      ],
      producingActions: [{ id: 'place', label: 'Place' }],
    }])
    expect(aggregate.behavior.domainErrors).toEqual([{
      key: errorKey,
      stableId: 'rejected',
      label: 'Order Rejected',
      code: 'REJECTED',
      message: 'Rejected',
      fields: [
        { name: 'reason', type: { kind: 'scalar', name: 'string' } },
        { name: 'lastStatus', type: { kind: 'optional', value: {
          kind: 'reference', name: 'Order Status', key: valueObjectKey(status),
        } } },
      ],
      returningActions: [{ id: 'place', label: 'Place' }],
    }])
    expect(action.outcomeLinks).toEqual([
      { kind: 'event', key: eventKey, stableId: 'placed', label: 'Order Placed' },
      { kind: 'error', key: errorKey, stableId: 'rejected', label: 'Order Rejected' },
    ])
  })

  it('deduplicates shared outcomes in first-reference order and keeps them scoped to action owners', () => {
    const source = model()
    const confirmed = { aggregate: sales, local: 'confirmed' }
    source.domainEvents.unshift({ id: confirmed, label: 'Order Confirmed', fields: [] })
    source.domainEvents.push({ id: { aggregate: sales, local: 'unused' }, label: 'Unused Event', fields: [] })
    source.domainErrors.push({
      id: { owner: { kind: 'aggregate', id: sales }, local: 'unused' },
      label: 'Unused Error', code: 'UNUSED', message: 'Unused', fields: [],
    })
    source.actions.push(
      { id: { owner: { kind: 'aggregate', id: sales }, local: 'confirm' }, label: 'Confirm',
        input: null, output: { kind: 'domainEvent', id: confirmed }, error: source.domainErrors[0]!.id },
      { id: { owner: { kind: 'aggregate', id: sales }, local: 'replace' }, label: 'Replace',
        input: null, output: { kind: 'domainEvent', id: source.domainEvents[1]!.id }, error: source.domainErrors[0]!.id },
    )

    const index = buildDomainIndex(source)
    const aggregate = index.selections.get(aggregateKey(sales))!
    const entity = index.selections.get(entityKey(line))!

    expect(aggregate.behavior.domainEvents.map((event) => event.stableId)).toEqual(['placed', 'confirmed'])
    expect(aggregate.behavior.domainEvents[0]!.producingActions).toEqual([
      { id: 'place', label: 'Place' },
      { id: 'replace', label: 'Replace' },
    ])
    expect(aggregate.behavior.domainEvents[1]!.producingActions).toEqual([
      { id: 'confirm', label: 'Confirm' },
    ])
    expect(aggregate.behavior.domainErrors).toHaveLength(1)
    expect(aggregate.behavior.domainErrors[0]!.returningActions.map((action) => action.id)).toEqual([
      'place', 'confirm', 'replace',
    ])
    expect(aggregate.behavior.domainEvents.some((event) => event.label === 'Unused Event')).toBe(false)
    expect(aggregate.behavior.domainErrors.some((error) => error.label === 'Unused Error')).toBe(false)
    expect(entity.behavior.domainEvents).toEqual([])
    expect(entity.behavior.domainErrors).toEqual([])
    expect(entity.behavior.actions[0]!.outcomeLinks).toEqual([])
  })

  it('indexes decisions for every owner kind and retains resolved metadata', () => {
    const source = model()
    const implementation = { kind: 'rust' as const }
    source.decisions.push(
      { id: { owner: { kind: 'aggregate', id: sales }, local: 'route' }, label: 'Route',
        input: { kind: 'valueObject', id: money }, output: { kind: 'valueObject', id: status }, implementation },
      { id: { owner: { kind: 'entity', id: line }, local: 'validate' }, label: 'Validate',
        input: { kind: 'valueObject', id: note }, output: { kind: 'valueObject', id: money }, implementation: { kind: 'rust' } },
      { id: { owner: { kind: 'valueObject', id: status }, local: 'advance' }, label: 'Advance',
        input: { kind: 'valueObject', id: status }, output: { kind: 'valueObject', id: note }, implementation: { kind: 'rust' } },
      { id: { owner: { kind: 'domainService', id: checkout }, local: 'review' }, label: 'Review',
        input: { kind: 'valueObject', id: money }, output: { kind: 'valueObject', id: status }, implementation: { kind: 'rust' } },
    )

    const index = buildDomainIndex(source)
    const aggregateDecision = index.selections.get(aggregateKey(sales))!.behavior.decisions[0]!

    expect(aggregateDecision).toMatchObject({
      id: 'route',
      label: 'Route',
      input: { kind: 'reference', name: 'Money', key: valueObjectKey(money) },
      output: { kind: 'reference', name: 'Order Status', key: valueObjectKey(status) },
    })
    expect(aggregateDecision.implementation).toBe(implementation)
    expect(index.selections.get(entityKey(line))!.behavior.decisions.map((item) => item.id)).toEqual(['validate'])
    expect(index.selections.get(valueObjectKey(status))!.behavior.decisions.map((item) => item.id)).toEqual(['advance'])
    expect(index.selections.get(serviceKey(checkout))!.behavior.decisions.map((item) => item.id)).toEqual(['review'])
    expect([...index.selections.values()].every((selection) => Array.isArray(selection.behavior.decisions))).toBe(true)
  })

  it('preserves interleaved decision projection order per owner', () => {
    const source = model()
    source.decisions.push(
      { id: { owner: { kind: 'aggregate', id: sales }, local: 'second' }, label: 'Second',
        input: { kind: 'valueObject', id: money }, output: { kind: 'valueObject', id: status }, implementation: { kind: 'rust' } },
      { id: { owner: { kind: 'entity', id: line }, local: 'entity' }, label: 'Entity',
        input: { kind: 'valueObject', id: note }, output: { kind: 'valueObject', id: money }, implementation: { kind: 'rust' } },
      { id: { owner: { kind: 'aggregate', id: sales }, local: 'first' }, label: 'First',
        input: { kind: 'valueObject', id: status }, output: { kind: 'valueObject', id: money }, implementation: { kind: 'rust' } },
    )

    const index = buildDomainIndex(source)

    expect(index.selections.get(aggregateKey(sales))!.behavior.decisions.map((item) => item.id)).toEqual([
      'second', 'first',
    ])
    expect(index.selections.get(entityKey(line))!.behavior.decisions.map((item) => item.id)).toEqual(['entity'])
  })

  it('rejects an unresolved decision owner', () => {
    const invalid = model()
    invalid.decisions.push({
      id: { owner: { kind: 'aggregate', id: { context: 'sales', local: 'missing' } }, local: 'route' },
      label: 'Route', input: { kind: 'valueObject', id: money }, output: { kind: 'valueObject', id: status },
      implementation: { kind: 'rust' },
    })

    expect(() => buildDomainIndex(invalid)).toThrow('Unresolved decision owner')
  })

  it('rejects unresolved decision input and output Value Objects', () => {
    const missing = { ...money, local: 'missing' }
    const invalidInput = model()
    invalidInput.decisions.push({
      id: { owner: { kind: 'aggregate', id: sales }, local: 'route' }, label: 'Route',
      input: { kind: 'valueObject', id: missing }, output: { kind: 'valueObject', id: status },
      implementation: { kind: 'rust' },
    })
    expect(() => buildDomainIndex(invalidInput)).toThrow('Unresolved decision input value object')

    const invalidOutput = model()
    invalidOutput.decisions.push({
      id: { owner: { kind: 'aggregate', id: sales }, local: 'route' }, label: 'Route',
      input: { kind: 'valueObject', id: money }, output: { kind: 'valueObject', id: missing },
      implementation: { kind: 'rust' },
    })
    expect(() => buildDomainIndex(invalidOutput)).toThrow('Unresolved decision output value object')
  })

  it.each(['input', 'output'] as const)('rejects a malformed decision %s kind', (role) => {
    const invalid = model()
    invalid.decisions.push({
      id: { owner: { kind: 'aggregate', id: sales }, local: 'route' }, label: 'Route',
      input: { kind: 'valueObject', id: money }, output: { kind: 'valueObject', id: status },
      implementation: { kind: 'rust' },
    })
    invalid.decisions[0]![role] = { kind: 'scalar', scalar: 'u64' } as never

    expect(() => buildDomainIndex(invalid)).toThrow(`Invalid decision ${role} kind: scalar`)
  })

  it('rejects duplicate owner-local Decision IDs', () => {
    const invalid = model()
    const decision = {
      id: { owner: { kind: 'aggregate' as const, id: sales }, local: 'route' }, label: 'Route',
      input: { kind: 'valueObject' as const, id: money }, output: { kind: 'valueObject' as const, id: status },
      implementation: { kind: 'rust' as const },
    }
    invalid.decisions.push(decision, { ...decision })

    expect(() => buildDomainIndex(invalid)).toThrow('Duplicate behavior key')
  })

  it('keeps actions, queries, invariants, and lifecycle unchanged when decisions are present', () => {
    const source = modelWithLifecycle()
    source.decisions.push({
      id: { owner: { kind: 'entity', id: line }, local: 'validate' }, label: 'Validate',
      input: { kind: 'valueObject', id: note }, output: { kind: 'valueObject', id: money },
      implementation: { kind: 'rust' },
    })

    const index = buildDomainIndex(source)
    const aggregate = index.selections.get(aggregateKey(sales))!
    const entity = index.selections.get(entityKey(line))!

    expect(aggregate.behavior.actions.map((item) => item.id)).toEqual(['place'])
    expect(aggregate.behavior.queries.map((item) => item.id)).toEqual(['find'])
    expect(aggregate.behavior.invariants.map((item) => item.id)).toEqual(['hasLines'])
    expect(entity.behavior.actions.map((item) => item.id)).toEqual(['clear', 'restore'])
    expect(entity.lifecycle!.transitions.map((item) => item.action.id)).toEqual(['clear', 'restore'])
    expect(entity.lifecycle!.transitions[0]!.action).toBe(entity.behavior.actions[0])
  })

  it('omits lifecycle presentation for entities without lifecycle metadata', () => {
    const entity = buildDomainIndex(model()).selections.get(entityKey(line))!

    expect(entity).not.toHaveProperty('lifecycle')
  })

  it('indexes resolved lifecycle metadata in source order', () => {
    const entity = buildDomainIndex(modelWithLifecycle()).selections.get(entityKey(line))!
    const lifecycle = entity.lifecycle!

    expect(lifecycle).toMatchObject({
      id: 'lineLifecycle',
      label: 'Order line lifecycle',
      initial: { id: 'active', label: 'Active' },
    })
    expect(lifecycle.states.map((state) => state.id)).toEqual(['active', 'cleared'])
    expect(lifecycle.initial).toBe(lifecycle.states[0])
    expect(lifecycle.transitions.map((transition) => [
      transition.source.id,
      transition.action.id,
      transition.target.id,
    ])).toEqual([
      ['active', 'clear', 'cleared'],
      ['cleared', 'restore', 'active'],
    ])
    expect(lifecycle.transitions[0]!.action).toBe(entity.behavior.actions[0])
    expect(lifecycle.transitions[1]!.action).toBe(entity.behavior.actions[1])
  })

  it('rejects duplicate lifecycle state IDs', () => {
    const invalid = modelWithLifecycle()
    invalid.entities[0]!.lifecycle!.states.push({ id: 'active', label: 'Active again' })

    expect(() => buildDomainIndex(invalid)).toThrow(
      'Duplicate lifecycle state id for Order Line: active',
    )
  })

  it('rejects an unknown lifecycle initial state', () => {
    const invalid = modelWithLifecycle()
    invalid.entities[0]!.lifecycle!.initial = 'missing'

    expect(() => buildDomainIndex(invalid)).toThrow(
      'Unresolved lifecycle initial state for Order Line: missing',
    )
  })

  it('rejects an unknown lifecycle transition source', () => {
    const invalid = modelWithLifecycle()
    invalid.entities[0]!.lifecycle!.transitions[0]!.source = 'missing'

    expect(() => buildDomainIndex(invalid)).toThrow(
      'Unresolved lifecycle source state for Order Line: missing',
    )
  })

  it('rejects an unknown lifecycle transition target', () => {
    const invalid = modelWithLifecycle()
    invalid.entities[0]!.lifecycle!.transitions[0]!.target = 'missing'

    expect(() => buildDomainIndex(invalid)).toThrow(
      'Unresolved lifecycle target state for Order Line: missing',
    )
  })

  it('rejects an unknown lifecycle transition action', () => {
    const invalid = modelWithLifecycle()
    invalid.entities[0]!.lifecycle!.transitions[0]!.action = {
      owner: { kind: 'entity', id: line },
      local: 'missing',
    }

    expect(() => buildDomainIndex(invalid)).toThrow('Unresolved lifecycle action for Order Line')
  })

  it('rejects a lifecycle transition action owned by another selection', () => {
    const invalid = modelWithLifecycle()
    invalid.entities[0]!.lifecycle!.transitions[0]!.action = invalid.actions[0]!.id

    expect(() => buildDomainIndex(invalid)).toThrow('Lifecycle action owner mismatch for Order Line')
  })

  it('falls back to context selection and rejects broken links', () => {
    const empty = model()
    empty.aggregates = []
    empty.entities = []
    empty.domainIdentities = []
    empty.valueObjects = []
    empty.domainServices = []
    empty.domainCommands = []
    empty.domainEvents = []
    empty.domainErrors = []
    empty.actions = []
    empty.decisions = []
    empty.queries = []
    empty.invariants = []
    expect(buildDomainIndex(empty).initialSelection).toBe(contextKey('sales'))

    const broken = model()
    broken.aggregates[0] = { ...broken.aggregates[0]!, root: { aggregate: sales, local: 'missing' } }
    expect(() => buildDomainIndex(broken)).toThrow('Unresolved aggregate root')

    const brokenReference = model()
    brokenReference.entities[1]!.fields[2] = {
      name: 'status',
      value: { kind: 'valueObject', id: { ...status, local: 'missing' } },
    }
    expect(() => buildDomainIndex(brokenReference)).toThrow('Unresolved value object')
  })
})
