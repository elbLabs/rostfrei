import type {
  DataDefinition,
  DisplayType,
  DomainIndex,
  DomainKey,
  PresentationField,
  SelectionKind,
} from './index'

export type DomainGraphEdgeKind = 'ownership' | 'root' | 'field' | 'input' | 'output' | 'error'

export interface DomainGraphNode {
  key: DomainKey
  label: string
  kind: SelectionKind
  root: boolean
  distance: number
  selected: boolean
}

export interface DomainGraphEdge {
  id: string
  source: DomainKey
  target: DomainKey
  kind: DomainGraphEdgeKind
  label?: string
}

export interface FocusedDomainGraph {
  nodes: DomainGraphNode[]
  edges: DomainGraphEdge[]
}

interface EdgeCandidate {
  source: DomainKey
  target: DomainKey
  kind: DomainGraphEdgeKind
  label?: string
}

export function buildFocusedGraph(
  index: DomainIndex,
  selectedKey: DomainKey,
  depth = 1,
): FocusedDomainGraph {
  if (!index.selections.has(selectedKey)) return { nodes: [], edges: [] }

  const candidates: EdgeCandidate[] = []
  const add = (source: DomainKey, target: DomainKey, kind: DomainGraphEdgeKind, label?: string) => {
    if (index.selections.has(source) && index.selections.has(target)) {
      candidates.push({ source, target, kind, ...(label === undefined ? {} : { label }) })
    }
  }

  for (const [child, parent] of index.parentKeys) add(parent, child, 'ownership')

  for (const selection of index.selections.values()) {
    if (selection.data.kind === 'aggregate') add(selection.key, selection.data.rootKey, 'root')
    for (const field of relationshipFields(selection.data)) {
      for (const target of referencedKeys(field.type)) add(selection.key, target, 'field', field.name)
    }

    const behavior = index.behaviorByOwner.get(selection.key) ?? selection.behavior
    for (const action of behavior.actions) {
      addTypeEdges(add, selection.key, action.input, 'input', action.label)
      addTypeEdges(add, selection.key, action.output, 'output', action.label)
      addTypeEdges(add, selection.key, action.error, 'error', action.label)
    }
    for (const decision of behavior.decisions) {
      addTypeEdges(add, selection.key, decision.input, 'input', decision.label)
      addTypeEdges(add, selection.key, decision.output, 'output', decision.label)
    }
    for (const query of behavior.queries) {
      addTypeEdges(add, selection.key, query.input, 'input', query.label)
      addTypeEdges(add, selection.key, query.output, 'output', query.label)
    }
  }

  const edgeKey = (edge: EdgeCandidate) => JSON.stringify([edge.source, edge.target, edge.kind, edge.label ?? null])
  const unique = [...new Map(candidates.map((edge) => [edgeKey(edge), edge])).values()]
    .sort((left, right) => edgeKey(left).localeCompare(edgeKey(right)))
  const adjacency = new Map<DomainKey, DomainKey[]>()
  for (const edge of unique) {
    adjacency.set(edge.source, [...(adjacency.get(edge.source) ?? []), edge.target])
    adjacency.set(edge.target, [...(adjacency.get(edge.target) ?? []), edge.source])
  }
  for (const neighbors of adjacency.values()) neighbors.sort((left, right) => left.localeCompare(right))

  const maxDepth = Math.max(0, Math.floor(Number.isFinite(depth) ? depth : 1))
  const distances = new Map<DomainKey, number>([[selectedKey, 0]])
  const queue: DomainKey[] = [selectedKey]
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    const current = queue[cursor]!
    const distance = distances.get(current)!
    if (distance >= maxDepth) continue
    for (const neighbor of adjacency.get(current) ?? []) {
      if (distances.has(neighbor)) continue
      distances.set(neighbor, distance + 1)
      queue.push(neighbor)
    }
  }

  const nodes = [...distances]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, distance]): DomainGraphNode => {
      const selection = index.selections.get(key)!
      return { key, label: selection.label, kind: selection.kind, root: selection.root, distance, selected: key === selectedKey }
    })
  const edges = unique
    .filter((edge) => distances.has(edge.source) && distances.has(edge.target))
    .map((edge): DomainGraphEdge => ({ ...edge, id: edgeKey(edge) }))

  return { nodes, edges }
}

function relationshipFields(definition: DataDefinition): PresentationField[] {
  if (definition.kind === 'struct') return definition.fields
  if (definition.kind !== 'enum') return []
  return definition.variants.flatMap((variant) => {
    if (variant.shape === 'unit') return []
    return variant.fields.map((field) => ({
      name: `${variant.name}.${field.name}`,
      type: field.type,
    }))
  })
}

function referencedKeys(type: DisplayType | null): DomainKey[] {
  if (type === null) return []
  switch (type.kind) {
    case 'reference': return type.key === undefined ? [] : [type.key]
    case 'list': return referencedKeys(type.element)
    case 'optional': return referencedKeys(type.value)
    case 'scalar':
    case 'semanticScalar':
    case 'unit': return []
  }
}

function addTypeEdges(
  add: (source: DomainKey, target: DomainKey, kind: DomainGraphEdgeKind, label?: string) => void,
  source: DomainKey,
  type: DisplayType | null,
  kind: DomainGraphEdgeKind,
  label: string,
): void {
  for (const target of referencedKeys(type)) add(source, target, kind, label)
}
