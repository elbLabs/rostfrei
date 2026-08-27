import type {
  Action,
  ActionInput,
  ActionOutput,
  ActionOwnerId,
  AggregateId,
  CanonicalScalarType,
  Decision,
  DecisionImplementation,
  DecisionInput,
  DecisionOutput,
  DecisionOwnerId,
  DomainErrorId,
  DomainIdentityId,
  DomainModel,
  DomainServiceId,
  Entity,
  EntityId,
  EntityLifecycle,
  EntityLifecycleState,
  Field,
  FieldValue,
  InvariantOwnerId,
  QueryInput,
  QueryOutput,
  ScalarType,
  ValueObject,
  ValueObjectId,
  ValueObjectOwnerId,
} from './schema'

declare const domainKeyBrand: unique symbol
export type DomainKey = string & { readonly [domainKeyBrand]: true }

export type SelectionKind =
  | 'context'
  | 'aggregate'
  | 'entity'
  | 'identity'
  | 'valueObject'
  | 'domainService'

export function contextKey(id: string): DomainKey {
  return key(['context', id])
}

export const boundedContextKey = contextKey

export function aggregateKey(id: AggregateId): DomainKey {
  return key(['aggregate', id.context, id.local])
}

export function entityKey(id: EntityId): DomainKey {
  return key(['entity', id.aggregate.context, id.aggregate.local, id.local])
}

export function identityKey(id: DomainIdentityId): DomainKey {
  return key(['identity', id.owner.aggregate.context, id.owner.aggregate.local, id.owner.local])
}

export function valueObjectKey(id: ValueObjectId): DomainKey {
  return key(['valueObject', ...ownerParts(id.owner), id.local])
}

export function serviceKey(id: DomainServiceId): DomainKey {
  return key(['domainService', id.context, id.local])
}

export const domainServiceKey = serviceKey

export function valueObjectOwnerKey(owner: ValueObjectOwnerId): DomainKey {
  switch (owner.kind) {
    case 'boundedContext': return contextKey(owner.id)
    case 'aggregate': return aggregateKey(owner.id)
    case 'entity': return entityKey(owner.id)
  }
}

export function actionOwnerKey(owner: ActionOwnerId): DomainKey {
  switch (owner.kind) {
    case 'aggregate': return aggregateKey(owner.id)
    case 'domainService': return serviceKey(owner.id)
    case 'entity': return entityKey(owner.id)
    case 'valueObject': return valueObjectKey(owner.id)
  }
}

export function decisionOwnerKey(owner: DecisionOwnerId): DomainKey {
  switch (owner.kind) {
    case 'aggregate': return aggregateKey(owner.id)
    case 'domainService': return serviceKey(owner.id)
    case 'entity': return entityKey(owner.id)
    case 'valueObject': return valueObjectKey(owner.id)
  }
}

export function invariantOwnerKey(owner: InvariantOwnerId): DomainKey {
  switch (owner.kind) {
    case 'aggregate': return aggregateKey(owner.id)
    case 'entity': return entityKey(owner.id)
    case 'valueObject': return valueObjectKey(owner.id)
  }
}

export type DisplayType =
  | { kind: 'scalar'; name: CanonicalScalarType }
  | { kind: 'semanticScalar'; id: string; name: string; representation: CanonicalScalarType }
  | { kind: 'unit'; name: '()' }
  | { kind: 'reference'; name: string; key?: DomainKey }
  | { kind: 'list'; element: DisplayType }
  | { kind: 'optional'; value: DisplayType }

export interface PresentationField {
  name: string
  type: DisplayType
}

export interface PresentationVariant {
  name: string
  shape: 'unit' | 'tuple' | 'struct'
  fields: PresentationField[]
}

export type PresentationOutcomeKey = string

export interface PresentationActionReference {
  id: string
  label: string
}

export interface PresentationActionOutcomeLink {
  kind: 'event' | 'error'
  key: PresentationOutcomeKey
  stableId: string
  label: string
}

export interface PresentationDomainEvent {
  key: PresentationOutcomeKey
  stableId: string
  label: string
  fields: PresentationField[]
  producingActions: PresentationActionReference[]
}

export interface PresentationDomainError {
  key: PresentationOutcomeKey
  stableId: string
  label: string
  code: string
  message: string
  fields: PresentationField[]
  returningActions: PresentationActionReference[]
}

export type DataDefinition =
  | { kind: 'context' }
  | { kind: 'aggregate'; rootKey: DomainKey }
  | { kind: 'struct'; fields: PresentationField[] }
  | { kind: 'enum'; variants: PresentationVariant[] }

export interface PresentationAction {
  id: string
  label: string
  visibility: 'Public' | 'Internal'
  input: DisplayType | null
  output: DisplayType
  error: DisplayType | null
  outcomeLinks: PresentationActionOutcomeLink[]
}

export interface PresentationDecision {
  id: string
  label: string
  input: DisplayType
  output: DisplayType
  implementation: DecisionImplementation
}

export interface PresentationLifecycleTransition {
  source: EntityLifecycleState
  action: PresentationAction
  target: EntityLifecycleState
}

export interface PresentationLifecycle {
  id: string
  label: string
  states: EntityLifecycleState[]
  initial: EntityLifecycleState
  transitions: PresentationLifecycleTransition[]
}

export interface PresentationQuery {
  id: string
  label: string
  input: DisplayType | null
  output: DisplayType
}

export interface PresentationInvariant {
  id: string
  label: string
}

export interface PresentationBehavior {
  actions: PresentationAction[]
  decisions: PresentationDecision[]
  queries: PresentationQuery[]
  invariants: PresentationInvariant[]
  domainEvents: PresentationDomainEvent[]
  domainErrors: PresentationDomainError[]
}

export interface PresentationSelection {
  key: DomainKey
  kind: SelectionKind
  label: string
  stableId: string
  ownerLabel?: string
  ownerKey?: DomainKey
  rustName?: string
  root: boolean
  data: DataDefinition
  behavior: PresentationBehavior
  lifecycle?: PresentationLifecycle
}

export interface SidebarTreeNode {
  key: DomainKey
  kind: Exclude<SelectionKind, 'identity'>
  label: string
  root: boolean
  children: SidebarTreeNode[]
}

export interface Breadcrumb {
  key: DomainKey
  kind: SelectionKind
  label: string
}

export interface DomainIndex {
  selections: Map<DomainKey, PresentationSelection>
  contexts: PresentationSelection[]
  aggregates: PresentationSelection[]
  entities: PresentationSelection[]
  rootEntities: PresentationSelection[]
  identities: PresentationSelection[]
  valueObjects: PresentationSelection[]
  domainServices: PresentationSelection[]
  parentKeys: Map<DomainKey, DomainKey>
  sidebar: SidebarTreeNode[]
  behaviorByOwner: Map<DomainKey, PresentationBehavior>
  initialSelection: DomainKey | null
}

export function getBreadcrumbTrail(index: DomainIndex, selectedKey: DomainKey): Breadcrumb[] {
  const result: Breadcrumb[] = []
  const visited = new Set<DomainKey>()
  let current: DomainKey | undefined = selectedKey
  while (current) {
    if (visited.has(current)) throw new Error(`Cyclic parent chain at ${current}`)
    visited.add(current)
    const selection = index.selections.get(current)
    if (!selection) throw new Error(`Unknown selection key ${current}`)
    result.unshift({ key: current, kind: selection.kind, label: selection.label })
    current = index.parentKeys.get(current)
  }
  return result
}

export function buildDomainIndex(model: DomainModel): DomainIndex {
  const selections = new Map<DomainKey, PresentationSelection>()
  const parentKeys = new Map<DomainKey, DomainKey>()
  const contexts: PresentationSelection[] = []
  const aggregates: PresentationSelection[] = []
  const entities: PresentationSelection[] = []
  const rootEntities: PresentationSelection[] = []
  const identities: PresentationSelection[] = []
  const valueObjects: PresentationSelection[] = []
  const domainServices: PresentationSelection[] = []
  const emptyBehavior = (): PresentationBehavior => ({
    actions: [], decisions: [], queries: [], invariants: [], domainEvents: [], domainErrors: [],
  })

  const add = (selection: PresentationSelection, collection: PresentationSelection[]) => {
    if (selections.has(selection.key)) throw new Error(`Duplicate domain key ${selection.key}`)
    selections.set(selection.key, selection)
    collection.push(selection)
    if (selection.ownerKey) parentKeys.set(selection.key, selection.ownerKey)
  }
  const requireSelection = (target: DomainKey, description: string) => {
    const selection = selections.get(target)
    if (!selection) throw new Error(`Unresolved ${description}: ${target}`)
    return selection
  }

  for (const context of model.boundedContexts) {
    add({ key: contextKey(context.id), kind: 'context', label: context.label, stableId: context.id,
      rustName: undefined, root: false, data: { kind: 'context' }, behavior: emptyBehavior() }, contexts)
  }
  for (const aggregate of model.aggregates) {
    const ownerKey = contextKey(aggregate.id.context)
    const owner = requireSelection(ownerKey, 'aggregate context')
    add({ key: aggregateKey(aggregate.id), kind: 'aggregate', label: aggregate.label,
      stableId: aggregate.id.local, ownerLabel: owner.label, ownerKey, rustName: undefined, root: false,
      data: { kind: 'aggregate', rootKey: entityKey(aggregate.root) }, behavior: emptyBehavior() }, aggregates)
  }
  for (const entity of model.entities) {
    const ownerKey = aggregateKey(entity.id.aggregate)
    const owner = requireSelection(ownerKey, 'entity aggregate')
    const aggregate = model.aggregates.find((item) => aggregateKey(item.id) === ownerKey)
    const root = aggregate !== undefined && entityKey(aggregate.root) === entityKey(entity.id)
    const selection: PresentationSelection = { key: entityKey(entity.id), kind: 'entity', label: entity.label,
      stableId: entity.id.local, ownerLabel: owner.label, ownerKey, rustName: undefined, root,
      data: { kind: 'struct', fields: [] }, behavior: emptyBehavior() }
    add(selection, entities)
    if (root) rootEntities.push(selection)
  }
  for (const identity of model.domainIdentities) {
    const ownerKey = entityKey(identity.id.owner)
    const owner = requireSelection(ownerKey, 'identity owner')
    add({ key: identityKey(identity.id), kind: 'identity', label: `Identity of ${owner.label}`,
      stableId: identity.id.owner.local, ownerLabel: owner.label, ownerKey, rustName: undefined, root: false,
      data: { kind: 'struct', fields: [{ name: 'value', type: scalarDisplayType(identity.scalar) }] },
      behavior: emptyBehavior() }, identities)
  }
  for (const valueObject of model.valueObjects) {
    const ownerKey = valueObjectOwnerKey(valueObject.id.owner)
    const owner = requireSelection(ownerKey, 'value object owner')
    add({ key: valueObjectKey(valueObject.id), kind: 'valueObject', label: valueObject.label,
      stableId: valueObject.id.local, ownerLabel: owner.label, ownerKey, rustName: undefined, root: false,
      data: initialValueObjectData(valueObject), behavior: emptyBehavior() }, valueObjects)
  }
  for (const service of model.domainServices) {
    const ownerKey = contextKey(service.id.context)
    const owner = requireSelection(ownerKey, 'domain service context')
    add({ key: serviceKey(service.id), kind: 'domainService', label: service.label,
      stableId: service.id.local, ownerLabel: owner.label, ownerKey, rustName: undefined, root: false,
      data: { kind: 'struct', fields: [] }, behavior: emptyBehavior() }, domainServices)
  }

  for (const aggregate of model.aggregates) {
    const root = requireSelection(entityKey(aggregate.root), 'aggregate root')
    if (root.kind !== 'entity' || root.ownerKey !== aggregateKey(aggregate.id)) {
      throw new Error(`Unresolved aggregate root for ${aggregate.label}: ${entityKey(aggregate.root)}`)
    }
  }

  uniqueLabels(model.domainCommands, (item) => commandKey(item.id), 'domain command')
  const eventLabels = uniqueLabels(model.domainEvents, (item) => eventKey(item.id), 'domain event')
  const errorLabels = uniqueLabels(model.domainErrors, (item) => errorKey(item.id), 'domain error')

  const reference = (value: FieldValue | ActionInput | ActionOutput | QueryInput | QueryOutput): DisplayType => {
    switch (value.kind) {
      case 'scalar': return scalarDisplayType(value.scalar)
      case 'list': return { kind: 'list', element: reference(value.element) }
      case 'optional': return { kind: 'optional', value: reference(value.value) }
      case 'identity':
      case 'domainIdentity': return linked(identityKey(value.id), 'identity')
      case 'entity': return linked(entityKey(value.id), 'entity')
      case 'valueObject': return linked(valueObjectKey(value.id), 'value object')
      case 'aggregateReference': return linked(aggregateKey(value.aggregate), 'aggregate reference')
      case 'domainEvent': return named(eventLabels, eventKey(value.id), 'domain event')
    }
  }
  const boundaryReference = (
    value: ActionInput | ActionOutput | QueryInput | QueryOutput,
    boundary: 'action' | 'query',
  ): DisplayType => {
    if (value.kind === 'scalar' && typeof (value as { scalar: unknown }).scalar !== 'string') {
      throw new Error(`Invalid ${boundary} scalar: semantic scalars require a modeled field or Domain Identity`)
    }
    if (value.kind === 'list') return { kind: 'list', element: boundaryReference(value.element, boundary) }
    if (value.kind === 'optional') return { kind: 'optional', value: boundaryReference(value.value, boundary) }
    return reference(value)
  }
  const linked = (target: DomainKey, description: string): DisplayType => {
    const selection = requireSelection(target, description)
    return { kind: 'reference', name: selection.label || selection.stableId, key: target }
  }
  const decisionValueObject = (value: DecisionInput | DecisionOutput, role: 'input' | 'output'): DisplayType => {
    if ((value as { kind: unknown }).kind !== 'valueObject') {
      throw new Error(`Invalid decision ${role} kind: ${String((value as { kind: unknown }).kind)}`)
    }
    return linked(valueObjectKey(value.id), `decision ${role} value object`)
  }
  const fields = (items: Field[]) => items.map((field) => ({ name: field.name, type: reference(field.value) }))

  for (const entity of model.entities) {
    const selection = requireSelection(entityKey(entity.id), 'entity')
    selection.data = { kind: 'struct', fields: fields(entity.fields) }
    requireSelection(identityKey(entity.identity.id), 'entity identity')
  }
  for (const valueObject of model.valueObjects) {
    const selection = requireSelection(valueObjectKey(valueObject.id), 'value object')
    if (valueObject.fields !== undefined) {
      selection.data = { kind: 'struct', fields: fields(valueObject.fields) }
      continue
    }
    selection.data = { kind: 'enum', variants: presentationVariants(valueObject, fields) }
  }
  for (const command of model.domainCommands) {
    const owner = command.id.owner.kind === 'aggregate'
      ? aggregateKey(command.id.owner.id)
      : serviceKey(command.id.owner.id)
    requireSelection(owner, 'domain command owner')
    fields(command.fields)
  }
  const eventDefinitions = new Map<PresentationOutcomeKey, PresentationDomainEvent>()
  for (const event of model.domainEvents) {
    requireSelection(aggregateKey(event.id.aggregate), 'domain event aggregate')
    const itemKey = eventKey(event.id)
    eventDefinitions.set(itemKey, { key: itemKey, stableId: event.id.local, label: event.label,
      fields: fields(event.fields), producingActions: [] })
  }
  const errorDefinitions = new Map<PresentationOutcomeKey, PresentationDomainError>()
  for (const error of model.domainErrors) {
    requireSelection(actionOwnerKey(error.id.owner), 'domain error owner')
    const itemKey = errorKey(error.id)
    errorDefinitions.set(itemKey, { key: itemKey, stableId: error.id.local, label: error.label,
      code: error.code, message: error.message, fields: fields(error.fields), returningActions: [] })
  }

  const behaviorIds = new Set<string>()
  const actionsByKey = new Map<string, PresentationAction>()
  const eventCardsByOwner = new Map<DomainKey, Map<PresentationOutcomeKey, PresentationDomainEvent>>()
  const errorCardsByOwner = new Map<DomainKey, Map<PresentationOutcomeKey, PresentationDomainError>>()
  for (const action of model.actions) {
    const ownerKey = actionOwnerKey(action.id.owner)
    const owner = requireSelection(ownerKey, 'action owner')
    const itemKey = actionKey(action.id)
    assertUniqueBehavior(behaviorIds, itemKey)
    const presentation: PresentationAction = { id: action.id.local, label: action.label,
      visibility: action.id.owner.kind === 'aggregate' || action.id.owner.kind === 'domainService' ? 'Public' : 'Internal',
      input: action.input ? boundaryReference(action.input, 'action') : null,
      output: action.output ? boundaryReference(action.output, 'action') : { kind: 'unit', name: '()' },
      error: action.error ? named(errorLabels, errorKey(action.error), 'domain error') : null,
      outcomeLinks: [] }
    const actionReference = { id: presentation.id, label: presentation.label }
    const actionEventKeys = new Set<PresentationOutcomeKey>()
    if (action.output) collectDomainEventKeys(action.output, actionEventKeys)
    for (const eventOutcomeKey of actionEventKeys) {
      const definition = eventDefinitions.get(eventOutcomeKey)!
      let cards = eventCardsByOwner.get(ownerKey)
      if (!cards) eventCardsByOwner.set(ownerKey, cards = new Map())
      let card = cards.get(eventOutcomeKey)
      if (!card) {
        card = { ...definition, producingActions: [] }
        cards.set(eventOutcomeKey, card)
        owner.behavior.domainEvents.push(card)
      }
      card.producingActions.push(actionReference)
      presentation.outcomeLinks.push({ kind: 'event', key: card.key, stableId: card.stableId, label: card.label })
    }
    if (action.error) {
      const errorOutcomeKey = errorKey(action.error)
      const definition = errorDefinitions.get(errorOutcomeKey)!
      let cards = errorCardsByOwner.get(ownerKey)
      if (!cards) errorCardsByOwner.set(ownerKey, cards = new Map())
      let card = cards.get(errorOutcomeKey)
      if (!card) {
        card = { ...definition, returningActions: [] }
        cards.set(errorOutcomeKey, card)
        owner.behavior.domainErrors.push(card)
      }
      card.returningActions.push(actionReference)
      presentation.outcomeLinks.push({ kind: 'error', key: card.key, stableId: card.stableId, label: card.label })
    }
    owner.behavior.actions.push(presentation)
    actionsByKey.set(itemKey, presentation)
  }
  for (const decision of model.decisions) {
    const ownerKey = decisionOwnerKey(decision.id.owner)
    const owner = requireSelection(ownerKey, 'decision owner')
    assertUniqueBehavior(behaviorIds, decisionKey(decision.id))
    owner.behavior.decisions.push({ id: decision.id.local, label: decision.label,
      input: decisionValueObject(decision.input, 'input'),
      output: decisionValueObject(decision.output, 'output'), implementation: decision.implementation })
  }
  for (const query of model.queries) {
    const ownerKey = aggregateKey(query.id.aggregate)
    const owner = requireSelection(ownerKey, 'query aggregate')
    assertUniqueBehavior(behaviorIds, key(['query', ownerKey, query.id.local]))
    owner.behavior.queries.push({ id: query.id.local, label: query.label,
      input: query.input ? boundaryReference(query.input, 'query') : null,
      output: boundaryReference(query.output, 'query') })
  }
  for (const invariant of model.invariants) {
    const ownerKey = invariantOwnerKey(invariant.id.owner)
    const owner = requireSelection(ownerKey, 'invariant owner')
    assertUniqueBehavior(behaviorIds, key(['invariant', ownerKey, invariant.id.local]))
    owner.behavior.invariants.push({ id: invariant.id.local, label: invariant.label })
  }
  for (const entity of model.entities) {
    if (entity.lifecycle === undefined) continue
    const selection = requireSelection(entityKey(entity.id), 'entity')
    selection.lifecycle = buildPresentationLifecycle(entity, entity.lifecycle, actionsByKey)
  }

  const behaviorByOwner = new Map<DomainKey, PresentationBehavior>()
  for (const selection of selections.values()) behaviorByOwner.set(selection.key, selection.behavior)

  const node = (selection: PresentationSelection): SidebarTreeNode => ({
    key: selection.key, kind: selection.kind as SidebarTreeNode['kind'], label: selection.label,
    root: selection.root, children: [],
  })
  const nodes = new Map<DomainKey, SidebarTreeNode>()
  for (const selection of [...contexts, ...aggregates, ...entities, ...valueObjects, ...domainServices]) {
    nodes.set(selection.key, node(selection))
  }
  for (const context of contexts) {
    const contextNode = nodes.get(context.key)!
    appendChildren(contextNode, aggregates, context.key, nodes)
    appendChildren(contextNode, valueObjects, context.key, nodes)
    appendChildren(contextNode, domainServices, context.key, nodes)
    for (const aggregate of aggregates.filter((item) => item.ownerKey === context.key)) {
      const aggregateNode = nodes.get(aggregate.key)!
      const ownedEntities = entities.filter((item) => item.ownerKey === aggregate.key)
      for (const entity of [...ownedEntities.filter((item) => item.root), ...ownedEntities.filter((item) => !item.root)]) {
        const entityNode = nodes.get(entity.key)!
        appendChildren(entityNode, valueObjects, entity.key, nodes)
        aggregateNode.children.push(entityNode)
      }
      appendChildren(aggregateNode, valueObjects, aggregate.key, nodes)
    }
  }

  return { selections, contexts, aggregates, entities, rootEntities, identities, valueObjects,
    domainServices, parentKeys, sidebar: contexts.map((item) => nodes.get(item.key)!), behaviorByOwner,
    initialSelection: aggregates[0]?.key ?? contexts[0]?.key ?? null }
}

function initialValueObjectData(valueObject: ValueObject): DataDefinition {
  if (valueObject.fields !== undefined) return { kind: 'struct', fields: [] }
  return {
    kind: 'enum',
    variants: valueObject.variants.map((name) => ({ name, shape: 'unit', fields: [] })),
  }
}

function presentationVariants(
  valueObject: ValueObject,
  resolveFields: (fields: Field[]) => PresentationField[],
): PresentationVariant[] {
  if (valueObject.variants === undefined) {
    throw new Error(`Missing enum variants for ${valueObject.label}`)
  }
  if (valueObject.variantShapes === undefined) {
    return valueObject.variants.map((name) => ({ name, shape: 'unit', fields: [] }))
  }
  if (valueObject.variants.length !== valueObject.variantShapes.length) {
    throw new Error(
      `Variant shape count mismatch for ${valueObject.label}: expected ${valueObject.variants.length}, received ${valueObject.variantShapes.length}`,
    )
  }
  return valueObject.variants.map((name, index) => {
    const variantShape = valueObject.variantShapes[index]!
    if (variantShape.name !== name) {
      throw new Error(
        `Variant shape alignment mismatch for ${valueObject.label} at index ${index}: expected ${name}, received ${variantShape.name}`,
      )
    }
    return {
      name,
      shape: variantShape.kind,
      fields: variantShape.kind === 'unit' ? [] : resolveFields(variantShape.fields),
    }
  })
}

function scalarDisplayType(scalar: ScalarType): DisplayType {
  if (typeof scalar === 'string') return { kind: 'scalar', name: scalar }
  return { kind: 'semanticScalar', id: scalar.id, name: scalar.label, representation: scalar.representation }
}

function key(parts: readonly unknown[]): DomainKey {
  return JSON.stringify(parts) as DomainKey
}

function ownerParts(owner: ValueObjectOwnerId): unknown[] {
  switch (owner.kind) {
    case 'boundedContext': return ['context', owner.id]
    case 'aggregate': return ['aggregate', owner.id.context, owner.id.local]
    case 'entity': return ['entity', owner.id.aggregate.context, owner.id.aggregate.local, owner.id.local]
  }
}

function actionKey(id: Action['id']): string {
  return key(['action', actionOwnerKey(id.owner), id.local])
}

function decisionKey(id: Decision['id']): string {
  return key(['decision', decisionOwnerKey(id.owner), id.local])
}

function commandKey(id: { owner: { kind: string; id: AggregateId | DomainServiceId }; local: string }): string {
  const owner = id.owner.kind === 'aggregate' ? aggregateKey(id.owner.id as AggregateId) : serviceKey(id.owner.id as DomainServiceId)
  return key(['domainCommand', owner, id.local])
}

function eventKey(id: { aggregate: AggregateId; local: string }): string {
  return key(['domainEvent', aggregateKey(id.aggregate), id.local])
}

function collectDomainEventKeys(output: ActionOutput, result: Set<PresentationOutcomeKey>): void {
  switch (output.kind) {
    case 'domainEvent': result.add(eventKey(output.id)); return
    case 'list': collectDomainEventKeys(output.element, result); return
    case 'optional': collectDomainEventKeys(output.value, result); return
  }
}

function errorKey(id: DomainErrorId): string {
  return key(['domainError', actionOwnerKey(id.owner), id.local])
}

function uniqueLabels<T extends { label: string }>(items: T[], getKey: (item: T) => string, kind: string): Map<string, string> {
  const result = new Map<string, string>()
  for (const item of items) {
    const itemKey = getKey(item)
    if (result.has(itemKey)) throw new Error(`Duplicate ${kind} key ${itemKey}`)
    result.set(itemKey, item.label)
  }
  return result
}

function named(labels: Map<string, string>, itemKey: string, kind: string): DisplayType {
  const label = labels.get(itemKey)
  if (!label) throw new Error(`Unresolved ${kind}: ${itemKey}`)
  return { kind: 'reference', name: label }
}

function assertUniqueBehavior(ids: Set<string>, id: string): void {
  if (ids.has(id)) throw new Error(`Duplicate behavior key ${id}`)
  ids.add(id)
}

function buildPresentationLifecycle(entity: Entity, lifecycle: EntityLifecycle,
  actionsByKey: Map<string, PresentationAction>): PresentationLifecycle {
  const statesById = new Map<string, EntityLifecycleState>()
  for (const state of lifecycle.states) {
    if (statesById.has(state.id)) throw new Error(`Duplicate lifecycle state id for ${entity.label}: ${state.id}`)
    statesById.set(state.id, state)
  }
  const resolveState = (id: string, role: string): EntityLifecycleState => {
    const state = statesById.get(id)
    if (!state) throw new Error(`Unresolved lifecycle ${role} state for ${entity.label}: ${id}`)
    return state
  }
  const initial = resolveState(lifecycle.initial, 'initial')
  const ownerKey = entityKey(entity.id)
  const transitions = lifecycle.transitions.map((transition): PresentationLifecycleTransition => {
    const source = resolveState(transition.source, 'source')
    const target = resolveState(transition.target, 'target')
    const transitionActionKey = actionKey(transition.action)
    if (actionOwnerKey(transition.action.owner) !== ownerKey) {
      throw new Error(`Lifecycle action owner mismatch for ${entity.label}: ${transitionActionKey}`)
    }
    const action = actionsByKey.get(transitionActionKey)
    if (!action) throw new Error(`Unresolved lifecycle action for ${entity.label}: ${transitionActionKey}`)
    return { source, action, target }
  })
  return { id: lifecycle.id, label: lifecycle.label, states: [...lifecycle.states], initial, transitions }
}

function appendChildren(parent: SidebarTreeNode, selections: PresentationSelection[], ownerKey: DomainKey,
  nodes: Map<DomainKey, SidebarTreeNode>): void {
  for (const selection of selections.filter((item) => item.ownerKey === ownerKey)) {
    parent.children.push(nodes.get(selection.key)!)
  }
}
