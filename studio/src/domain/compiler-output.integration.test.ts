import { execFileSync } from 'node:child_process'
import { resolve } from 'node:path'

import { beforeAll, describe, expect, it } from 'vitest'

import {
  aggregateKey,
  buildDomainIndex,
  entityKey,
  identityKey,
  valueObjectKey,
  type DomainIndex,
} from './index'
import { parseDomainModel } from './schema'

const CARGO_TIMEOUT_MS = 120_000
const repositoryRoot = resolve(process.cwd(), '..')
const aggregateId = { context: 'bike-rental', local: 'rental-fleet' }
const rootId = { aggregate: aggregateId, local: 'rental-fleet-root' }
const bicycleId = { aggregate: aggregateId, local: 'bicycle' }
const ownedByAggregate = { kind: 'aggregate' as const, id: aggregateId }
const statusId = { owner: ownedByAggregate, local: 'bicycle-status' }
const availabilityId = { owner: ownedByAggregate, local: 'bicycle-availability' }
const decisionInputId = { owner: ownedByAggregate, local: 'rental-eligibility-input' }
const decisionOutputId = { owner: ownedByAggregate, local: 'rental-eligibility-decision' }

let index: DomainIndex

beforeAll(() => {
  const stdout = execFileSync('cargo', [
    'run', '--quiet', '--locked', '-p', 'bike-rental', '--bin', 'bike-rental-model',
  ], {
    cwd: repositoryRoot,
    encoding: 'utf8',
    timeout: CARGO_TIMEOUT_MS,
    maxBuffer: 10 * 1024 * 1024,
  })
  index = buildDomainIndex(parseDomainModel(JSON.parse(stdout)))
}, CARGO_TIMEOUT_MS + 10_000)

describe('Bike rental compiler output', () => {
  it('attaches the Rust decision to its aggregate', () => {
    const aggregate = index.selections.get(aggregateKey(aggregateId))!

    expect(aggregate.behavior.decisions).toHaveLength(1)
    expect(aggregate.behavior.decisions[0]).toMatchObject({
      id: 'assess-rental-eligibility',
      label: 'Assess rental eligibility',
      input: {
        kind: 'reference',
        name: 'Rental eligibility input',
        key: valueObjectKey(decisionInputId),
      },
      output: {
        kind: 'reference',
        name: 'Rental eligibility decision',
        key: valueObjectKey(decisionOutputId),
      },
      implementation: { kind: 'rust' },
    })
  })

  it('indexes the public aggregate actions, internal entity action, and query', () => {
    const aggregate = index.selections.get(aggregateKey(aggregateId))!
    const bicycle = index.selections.get(entityKey(bicycleId))!
    const rent = aggregate.behavior.actions.find((action) => action.id === 'rent-bicycle')!

    expect(aggregate.behavior.actions).toHaveLength(2)
    expect(rent).toMatchObject({
      id: 'rent-bicycle',
      label: 'Rent bicycle',
      visibility: 'Public',
      input: {
        kind: 'reference',
        name: 'Identity of Bicycle',
        key: identityKey({ owner: bicycleId }),
      },
      output: { kind: 'reference', name: 'Bicycle rented' },
      error: { kind: 'reference', name: 'Bicycle unavailable' },
    })

    expect(bicycle.behavior.actions).toHaveLength(1)
    expect(bicycle.behavior.actions[0]).toMatchObject({
      id: 'mark-rented',
      label: 'Mark rented',
      visibility: 'Internal',
      input: {
        kind: 'reference',
        name: 'Bicycle status',
        key: valueObjectKey(statusId),
      },
      output: { kind: 'unit', name: '()' },
      error: null,
    })

    expect(aggregate.behavior.queries).toHaveLength(1)
    expect(aggregate.behavior.queries[0]).toMatchObject({
      id: 'bicycle-availability',
      label: 'Bicycle availability',
      input: {
        kind: 'reference',
        name: 'Identity of Bicycle',
        key: identityKey({ owner: bicycleId }),
      },
      output: {
        kind: 'optional',
        value: {
          kind: 'reference',
          name: 'Bicycle availability',
          key: valueObjectKey(availabilityId),
        },
      },
    })
  })

  it('links the event and error to their producing action', () => {
    const aggregate = index.selections.get(aggregateKey(aggregateId))!
    const action = aggregate.behavior.actions.find((item) => item.id === 'rent-bicycle')!
    const event = aggregate.behavior.domainEvents.find((item) => item.stableId === 'bicycle-rented')!
    const error = aggregate.behavior.domainErrors.find((item) => item.stableId === 'bicycle-unavailable')!

    expect(event).toMatchObject({
      stableId: 'bicycle-rented',
      label: 'Bicycle rented',
      fields: [
        {
          name: 'fleet_id',
          type: {
            kind: 'reference',
            name: 'Identity of Rental fleet',
            key: identityKey({ owner: rootId }),
          },
        },
        {
          name: 'bicycle_id',
          type: {
            kind: 'reference',
            name: 'Identity of Bicycle',
            key: identityKey({ owner: bicycleId }),
          },
        },
      ],
      producingActions: [{ id: 'rent-bicycle', label: 'Rent bicycle' }],
    })
    expect(error).toMatchObject({
      stableId: 'bicycle-unavailable',
      label: 'Bicycle unavailable',
      code: 'BICYCLE_UNAVAILABLE',
      message: 'The requested bicycle cannot currently be rented.',
      returningActions: [{ id: 'rent-bicycle', label: 'Rent bicycle' }],
    })
    expect(action.outcomeLinks).toEqual([
      { kind: 'event', key: event.key, stableId: event.stableId, label: event.label },
      { kind: 'error', key: error.key, stableId: error.stableId, label: error.label },
    ])
  })

  it('normalizes the bicycle status enum for presentation', () => {
    expect(index.selections.get(valueObjectKey(statusId))!.data).toEqual({
      kind: 'enum',
      variants: [
        { name: 'Available', shape: 'unit', fields: [] },
        { name: 'Rented', shape: 'unit', fields: [] },
      ],
    })
  })
})
