import { describe, expect, it } from 'vitest'

import {
  aggregateKey,
  buildDomainIndex,
  contextKey,
  entityKey,
  type DomainKey,
  valueObjectKey,
} from './index'
import type { DomainModel } from './schema'
import { buildFocusedGraph } from './graph'

const aggregate = { context: 'sales', local: 'orders' }
const order = { aggregate, local: 'order' }
const line = { aggregate, local: 'line' }
const orderId = { owner: order }
const lineId = { owner: line }
const money = { owner: { kind: 'boundedContext' as const, id: 'sales' }, local: 'money' }
const adjustment = { owner: { kind: 'boundedContext' as const, id: 'sales' }, local: 'adjustment' }

function fixture(): DomainModel {
  return {
    boundedContexts: [{ id: 'sales', label: 'Sales' }],
    aggregates: [{ id: aggregate, label: 'Orders', root: order }],
    entities: [
      { id: order, label: 'Order', identity: { field: 'id', id: orderId }, fields: [
        { name: 'lines', value: { kind: 'optional', value: { kind: 'list', element: { kind: 'entity', id: line } } } },
        { name: 'total', value: { kind: 'valueObject', id: money } },
        { name: 'confirmedTotal', value: { kind: 'valueObject', id: money } },
      ] },
      { id: line, label: 'Line', identity: { field: 'id', id: lineId }, fields: [
        { name: 'price', value: { kind: 'valueObject', id: money } },
      ] },
    ],
    domainIdentities: [{ id: orderId, scalar: 'u64' }, { id: lineId, scalar: 'u64' }],
    valueObjects: [{ id: money, label: 'Money', fields: [] }],
    domainServices: [],
    domainCommands: [],
    domainEvents: [],
    domainErrors: [],
    actions: [{
      id: { owner: { kind: 'aggregate', id: aggregate }, local: 'quote' }, label: 'Quote',
      input: { kind: 'valueObject', id: money },
      output: { kind: 'optional', value: { kind: 'list', element: { kind: 'valueObject', id: money } } }, error: null,
    }],
    decisions: [],
    queries: [{
      id: { aggregate, local: 'find' }, label: 'Find',
      input: { kind: 'domainIdentity', id: orderId }, output: { kind: 'list', element: { kind: 'valueObject', id: money } },
    }],
    invariants: [],
  }
}

const edgeSummary = (selectedKey: DomainKey, depth = 1) => buildFocusedGraph(buildDomainIndex(fixture()), selectedKey, depth)
  .edges.map(({ source, target, kind, label }) => [source, target, kind, label])

describe('buildFocusedGraph', () => {
  it('derives ownership and the aggregate root relationship', () => {
    const edges = edgeSummary(aggregateKey(aggregate))
    expect(edges).toContainEqual([contextKey('sales'), aggregateKey(aggregate), 'ownership', undefined])
    expect(edges).toContainEqual([aggregateKey(aggregate), entityKey(order), 'root', undefined])
    expect(edges).toContainEqual([aggregateKey(aggregate), entityKey(order), 'ownership', undefined])
  })

  it('recursively extracts nested field references and includes inbound references', () => {
    const graph = buildFocusedGraph(buildDomainIndex(fixture()), entityKey(line))
    expect(graph.edges).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: entityKey(order), target: entityKey(line), kind: 'field', label: 'lines' }),
      expect.objectContaining({ source: entityKey(line), target: valueObjectKey(money), kind: 'field', label: 'price' }),
    ]))
    expect(graph.nodes.find((node) => node.key === entityKey(order))?.distance).toBe(1)
  })

  it('extracts keyed action and query contracts while omitting unkeyed references', () => {
    const edges = edgeSummary(aggregateKey(aggregate))
    expect(edges).toEqual(expect.arrayContaining([
      [aggregateKey(aggregate), valueObjectKey(money), 'input', 'Quote'],
      [aggregateKey(aggregate), valueObjectKey(money), 'output', 'Quote'],
      [aggregateKey(aggregate), valueObjectKey(money), 'output', 'Find'],
    ]))
  })

  it('extracts qualified tagged payload fields through nested wrappers and includes inbound edges', () => {
    const source = fixture()
    source.valueObjects.push({
      id: adjustment,
      label: 'Adjustment',
      variants: ['None', 'EmptyTuple', 'EmptyStruct', 'Pair', 'Changed'],
      variantShapes: [
        { name: 'None', kind: 'unit' },
        { name: 'EmptyTuple', kind: 'tuple', fields: [] },
        { name: 'EmptyStruct', kind: 'struct', fields: [] },
        { name: 'Pair', kind: 'tuple', fields: [
          { name: 'amount', value: { kind: 'optional', value: { kind: 'list', element: {
            kind: 'valueObject', id: money,
          } } } },
          { name: 'line', value: { kind: 'entity', id: line } },
        ] },
        { name: 'Changed', kind: 'struct', fields: [
          { name: 'amount', value: { kind: 'valueObject', id: money } },
          { name: 'amount', value: { kind: 'valueObject', id: money } },
        ] },
      ],
    })
    const index = buildDomainIndex(source)
    const adjustmentKey = valueObjectKey(adjustment)
    const graph = buildFocusedGraph(index, adjustmentKey)
    const taggedFields = graph.edges.filter((edge) => edge.source === adjustmentKey && edge.kind === 'field')

    expect(taggedFields).toEqual(expect.arrayContaining([
      expect.objectContaining({ target: valueObjectKey(money), label: 'Pair.amount' }),
      expect.objectContaining({ target: entityKey(line), label: 'Pair.line' }),
      expect.objectContaining({ target: valueObjectKey(money), label: 'Changed.amount' }),
    ]))
    expect(taggedFields.map((edge) => edge.label)).toHaveLength(3)
    expect(taggedFields.some((edge) => edge.label?.startsWith('None.'))).toBe(false)
    expect(taggedFields.some((edge) => edge.label?.startsWith('EmptyTuple.'))).toBe(false)
    expect(taggedFields.some((edge) => edge.label?.startsWith('EmptyStruct.'))).toBe(false)

    const inbound = buildFocusedGraph(index, valueObjectKey(money))
    expect(inbound.nodes.find((node) => node.key === adjustmentKey)?.distance).toBe(1)
    expect(inbound.edges).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: adjustmentKey, target: valueObjectKey(money), label: 'Pair.amount' }),
      expect.objectContaining({ source: adjustmentKey, target: valueObjectKey(money), label: 'Changed.amount' }),
    ]))
  })

  it('limits undirected traversal by depth', () => {
    const index = buildDomainIndex(fixture())
    expect(buildFocusedGraph(index, contextKey('sales'), 0).nodes.map((node) => node.key)).toEqual([contextKey('sales')])
    expect(buildFocusedGraph(index, contextKey('sales'), 1).nodes.some((node) => node.key === entityKey(line))).toBe(false)
    expect(buildFocusedGraph(index, contextKey('sales'), 2).nodes.find((node) => node.key === entityKey(line))?.distance).toBe(2)
  })

  it('deduplicates equivalent edges but retains distinct field labels', () => {
    const source = fixture()
    const root = source.entities[0]!
    root.fields.push({ ...root.fields[1]! })
    const graph = buildFocusedGraph(buildDomainIndex(source), entityKey(order))
    const moneyFields = graph.edges.filter((edge) => edge.source === entityKey(order) && edge.target === valueObjectKey(money) && edge.kind === 'field')
    expect(moneyFields.map((edge) => edge.label)).toEqual(['confirmedTotal', 'total'])
    expect(new Set(graph.edges.map((edge) => edge.id)).size).toBe(graph.edges.length)
  })
})
