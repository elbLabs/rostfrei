export type CanonicalScalarType =
  | 'bool'
  | 'string'
  | 'char'
  | 'f32'
  | 'f64'
  | 'i8'
  | 'i16'
  | 'i32'
  | 'i64'
  | 'i128'
  | 'isize'
  | 'u8'
  | 'u16'
  | 'u32'
  | 'u64'
  | 'u128'
  | 'usize'

export interface SemanticScalarType {
  kind: 'semantic'
  id: string
  label: string
  representation: CanonicalScalarType
}

export type ScalarType = CanonicalScalarType | SemanticScalarType

export interface AggregateId {
  context: string
  local: string
}

export interface EntityId {
  aggregate: AggregateId
  local: string
}

export interface DomainServiceId {
  context: string
  local: string
}

export interface DomainIdentityId {
  owner: EntityId
}

export type ValueObjectOwnerId =
  | { kind: 'boundedContext'; id: string }
  | { kind: 'aggregate'; id: AggregateId }
  | { kind: 'entity'; id: EntityId }

export interface ValueObjectId {
  owner: ValueObjectOwnerId
  local: string
}

export type ActionOwnerId =
  | { kind: 'aggregate'; id: AggregateId }
  | { kind: 'domainService'; id: DomainServiceId }
  | { kind: 'entity'; id: EntityId }
  | { kind: 'valueObject'; id: ValueObjectId }

export type DecisionOwnerId =
  | { kind: 'aggregate'; id: AggregateId }
  | { kind: 'domainService'; id: DomainServiceId }
  | { kind: 'entity'; id: EntityId }
  | { kind: 'valueObject'; id: ValueObjectId }

export interface DecisionId {
  owner: DecisionOwnerId
  local: string
}

export type DomainCommandOwnerId =
  | { kind: 'aggregate'; id: AggregateId }
  | { kind: 'domainService'; id: DomainServiceId }

export interface DomainCommandId {
  owner: DomainCommandOwnerId
  local: string
}

export type DomainErrorOwnerId =
  | { kind: 'aggregate'; id: AggregateId }
  | { kind: 'domainService'; id: DomainServiceId }
  | { kind: 'entity'; id: EntityId }
  | { kind: 'valueObject'; id: ValueObjectId }

export interface DomainErrorId {
  owner: DomainErrorOwnerId
  local: string
}

export interface DomainEventId {
  aggregate: AggregateId
  local: string
}

export interface QueryId {
  aggregate: AggregateId
  local: string
}

export type InvariantOwnerId =
  | { kind: 'aggregate'; id: AggregateId }
  | { kind: 'entity'; id: EntityId }
  | { kind: 'valueObject'; id: ValueObjectId }

export interface InvariantId {
  owner: InvariantOwnerId
  local: string
}

export interface ScalarReference {
  kind: 'scalar'
  scalar: ScalarType
}

export interface CanonicalScalarReference {
  kind: 'scalar'
  scalar: CanonicalScalarType
}

export interface ValueObjectReference {
  kind: 'valueObject'
  id: ValueObjectId
}

export interface ListReference<T> {
  kind: 'list'
  element: T
}

export interface OptionalReference<T> {
  kind: 'optional'
  value: T
}

export type FieldValue =
  | ScalarReference
  | { kind: 'identity'; id: DomainIdentityId }
  | { kind: 'entity'; id: EntityId }
  | ValueObjectReference
  | { kind: 'aggregateReference'; aggregate: AggregateId }
  | ListReference<FieldValue>
  | OptionalReference<FieldValue>

export interface Field {
  name: string
  value: FieldValue
}

export interface BoundedContext {
  id: string
  label: string
}

export interface Aggregate {
  id: AggregateId
  label: string
  root: EntityId
}

export interface EntityLifecycleState {
  id: string
  label: string
}

export interface EntityLifecycleTransition {
  source: string
  action: Action['id']
  target: string
}

export interface EntityLifecycle {
  id: string
  label: string
  states: EntityLifecycleState[]
  initial: string
  transitions: EntityLifecycleTransition[]
}

export interface Entity {
  id: EntityId
  label: string
  identity: {
    field: string
    id: DomainIdentityId
  }
  fields: Field[]
  lifecycle?: EntityLifecycle
}

export interface DomainIdentity {
  id: DomainIdentityId
  scalar: ScalarType
}

interface ValueObjectBase {
  id: ValueObjectId
  label: string
}

export type ValueObjectVariantShape =
  | { name: string; kind: 'unit'; fields?: never }
  | { name: string; kind: 'tuple'; fields: Field[] }
  | { name: string; kind: 'struct'; fields: Field[] }

export type ValueObject =
  | (ValueObjectBase & { fields: Field[]; variants?: never; variantShapes?: never })
  | (ValueObjectBase & { variants: string[]; fields?: never; variantShapes?: never })
  | (ValueObjectBase & { variants: string[]; variantShapes: ValueObjectVariantShape[]; fields?: never })

export interface DomainService {
  id: DomainServiceId
  label: string
}

export interface DomainCommand {
  id: DomainCommandId
  label: string
  fields: Field[]
}

export interface DomainEvent {
  id: DomainEventId
  label: string
  fields: Field[]
}

export interface DomainError {
  id: DomainErrorId
  label: string
  code: string
  message: string
  fields: Field[]
}

export type ActionInput =
  | CanonicalScalarReference
  | ValueObjectReference
  | { kind: 'domainCommand'; id: DomainCommandId }

export type ActionOutput =
  | CanonicalScalarReference
  | ValueObjectReference
  | { kind: 'domainEvent'; id: DomainEventId }
  | ListReference<ActionOutput>
  | OptionalReference<ActionOutput>

export interface Action {
  id: {
    owner: ActionOwnerId
    local: string
  }
  label: string
  input: ActionInput | null
  output: ActionOutput | null
  error: DomainErrorId | null
}

export type DecisionInput = ValueObjectReference

export type DecisionOutput = ValueObjectReference

export interface DecisionImplementation {
  kind: 'rust'
}

export interface Decision {
  id: DecisionId
  label: string
  input: DecisionInput
  output: DecisionOutput
  implementation: DecisionImplementation
}

export type QueryInput =
  | CanonicalScalarReference
  | ValueObjectReference
  | { kind: 'domainIdentity'; id: DomainIdentityId }

export type QueryOutput =
  | QueryInput
  | ListReference<QueryOutput>
  | OptionalReference<QueryOutput>

export interface Query {
  id: QueryId
  label: string
  input: QueryInput | null
  output: QueryOutput
}

export interface Invariant {
  id: InvariantId
  label: string
}

export interface DomainModel {
  boundedContexts: BoundedContext[]
  aggregates: Aggregate[]
  entities: Entity[]
  domainIdentities: DomainIdentity[]
  valueObjects: ValueObject[]
  domainServices: DomainService[]
  domainCommands: DomainCommand[]
  domainEvents: DomainEvent[]
  domainErrors: DomainError[]
  actions: Action[]
  decisions: Decision[]
  queries: Query[]
  invariants: Invariant[]
}

const COLLECTIONS = [
  'boundedContexts',
  'aggregates',
  'entities',
  'domainIdentities',
  'valueObjects',
  'domainServices',
  'domainCommands',
  'domainEvents',
  'domainErrors',
  'actions',
  'decisions',
  'queries',
  'invariants',
] as const

export function parseDomainModel(input: unknown): DomainModel {
  if (typeof input !== 'object' || input === null || Array.isArray(input)) {
    throw new TypeError('Domain model must be an object')
  }

  const model = input as Record<string, unknown>
  for (const collection of COLLECTIONS) {
    if (!Array.isArray(model[collection])) {
      throw new TypeError(`Domain model ${collection} must be an array`)
    }
  }

  return model as unknown as DomainModel
}
