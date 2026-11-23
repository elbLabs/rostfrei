import { MESSAGE_HEIGHT, MESSAGE_WIDTH } from "@/lib/graph"
import type { GraphCardFact } from "./graph-card"
import type { DomainInventory } from "./inventory-panel-data"

type RecordValue = Record<string, unknown>
export const record = (value: unknown): RecordValue =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as RecordValue)
    : {}
export const string = (value: unknown) =>
  typeof value === "string" ? value : undefined

export function aggregatePath(value: unknown) {
  const id = record(value)
  const context = string(id.context)
  const local = string(id.local)
  return context && local ? `${context}/${local}` : undefined
}

export function entityPath(value: unknown) {
  const id = record(value)
  const aggregate = aggregatePath(id.aggregate)
  const local = string(id.local)
  return aggregate && local ? `${aggregate}/${local}` : undefined
}

export type DomainKind = "context" | "aggregate" | "entity" | "event"
export const domainKindLabels: Record<DomainKind, string> = {
  context: "Bounded context",
  aggregate: "Aggregate",
  entity: "Entity",
  event: "Domain event",
}
export type DomainNode = {
  id: string
  kind: DomainKind
  label: string
  path: string
  value: unknown
  caption: string
}

type DomainLinkKind =
  | "aggregate-context"
  | "root-entity"
  | "owns-entity"
  | "declares-event"
  | "field-reference"
type DomainLink = { source: string; target: string; kind: DomainLinkKind }

function fieldTarget(value: unknown): string | undefined {
  const field = record(value)
  if (field.kind === "optional") return fieldTarget(field.value)
  if (field.kind === "list") return fieldTarget(field.element)
  if (field.kind === "aggregateReference") {
    const path = aggregatePath(field.aggregate)
    return path && `aggregate:${path}`
  }
  if (field.kind === "entity") {
    const path = entityPath(field.id)
    return path && `entity:${path}`
  }
}

export function layoutDomain(data: DomainInventory) {
  const nodes = new Map<string, DomainNode>()
  const links: DomainLink[] = []
  function add(
    kind: DomainKind,
    path: string | undefined,
    value: unknown,
    caption = "",
    parent?: string,
    relation?: DomainLinkKind
  ) {
    if (!path) return
    const id = `${kind}:${path}`
    const label = string(record(value).label) ?? path.split("/").at(-1) ?? path
    nodes.set(id, { id, kind, label, path, value, caption })
    if (parent && relation && nodes.has(parent))
      links.push({ source: parent, target: id, kind: relation })
  }

  for (const context of data.boundedContexts)
    add("context", context.id, context)
  for (const aggregate of data.aggregates)
    add(
      "aggregate",
      aggregatePath(aggregate.id),
      aggregate,
      "",
      `context:${aggregate.id.context}`,
      "aggregate-context"
    )
  for (const entity of data.entities) {
    const path = entityPath(entity.id)
    const parent = `aggregate:${aggregatePath(entity.id.aggregate)}`
    const root =
      !!path && entityPath(record(nodes.get(parent)?.value).root) === path
    add(
      "entity",
      path,
      entity,
      root ? "Aggregate root" : "",
      parent,
      root ? "root-entity" : "owns-entity"
    )
  }
  for (const value of data.domainEvents ?? []) {
    const id = record(record(value).id)
    add(
      "event",
      entityPath(id),
      value,
      "",
      `aggregate:${aggregatePath(id.aggregate)}`,
      "declares-event"
    )
  }
  // No relationships are inferred from labels or command registrations.
  for (const node of nodes.values()) {
    const fields = record(node.value).fields
    if (!Array.isArray(fields)) continue
    const targets = new Set(
      fields.map((field) => fieldTarget(record(field).value))
    )
    for (const target of targets)
      if (target && nodes.has(target))
        links.push({ source: node.id, target, kind: "field-reference" })
  }
  return { nodes, links }
}

export type DomainLayout = ReturnType<typeof layoutDomain>
type Point = { x: number; y: number }
type Rect = Point & { width: number; height: number }
export type DomainCard = Rect & {
  id: string
  kind: DomainKind
  label: string
  facts: GraphCardFact[]
  action: string
  navigates: boolean
}
export type DomainEdge = DomainLink & { path: string }
export type DomainHeading = Point & { id: string; label: string; width: number }

type Grid = { columns: number; width: number; height: number }
const headingHeight = 20
const headingSpace = 38
const gutter = 16
const railMargin = 32
const blockGap = 96

const center = ({ x, width }: Rect) => x + width / 2
const right = ({ x, width }: Rect) => x + width
const bottom = ({ y, height }: Rect) => y + height
const compare = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0)

function line(...points: [x: number, y: number][]) {
  // Round only the bends. Each approach still meets its card at right angles.
  let path = `M ${points[0].join(" ")}`
  for (let i = 1; i < points.length - 1; i++) {
    const [x, y] = points[i]
    const [px, py] = points[i - 1]
    const [nx, ny] = points[i + 1]
    const incoming = Math.hypot(px - x, py - y)
    const outgoing = Math.hypot(nx - x, ny - y)
    const radius = Math.min(9, incoming / 2, outgoing / 2)
    if (!radius) {
      path += ` L ${x} ${y}`
      continue
    }
    path += ` L ${x + ((px - x) * radius) / incoming} ${y + ((py - y) * radius) / incoming} Q ${x} ${y} ${x + ((nx - x) * radius) / outgoing} ${y + ((ny - y) * radius) / outgoing}`
  }
  return `${path} L ${points.at(-1)!.join(" ")}`
}

export function scopeDomain(
  model: DomainLayout,
  aggregate?: DomainNode,
  compact = false
) {
  const cardsById = new Map<string, DomainCard>()
  const edges: DomainEdge[] = []
  const headings: DomainHeading[] = []
  const columnGap = compact ? 34 : 68
  const rowGap = compact ? 40 : 58
  const cardWidth = compact ? 200 : MESSAGE_WIDTH
  const cardHeight = MESSAGE_HEIGHT
  const ownerWidth = cardWidth
  const rails = new Map<string, number[]>()
  const links = model.links
    .filter(
      ({ source, target }) => model.nodes.has(source) && model.nodes.has(target)
    )
    .sort(
      (a, b) =>
        compare(a.source, b.source) ||
        compare(a.target, b.target) ||
        compare(a.kind, b.kind)
    )
    .filter((link, i, sorted) => {
      const prev = sorted[i - 1]
      return (
        !prev ||
        prev.source !== link.source ||
        prev.target !== link.target ||
        prev.kind !== link.kind
      )
    })

  function place(node: DomainNode, x: number, y: number) {
    const value = record(node.value)
    const navigates = node.kind === "aggregate" && !aggregate
    const facts: GraphCardFact[] = []
    if (node.kind === "context") {
      const count = links.filter(
        ({ source, kind }) => source === node.id && kind === "aggregate-context"
      ).length
      facts.push(["id", node.path], ["aggregates", String(count)])
    } else if (node.kind === "aggregate") {
      const context = string(record(value.id).context)
      const rootPath = entityPath(value.root)
      const root =
        (rootPath && model.nodes.get(`entity:${rootPath}`)?.label) ??
        string(record(value.root).local)
      if (context)
        facts.push([
          "context",
          model.nodes.get(`context:${context}`)?.label ?? context,
        ])
      if (root) facts.push(["root", root])
    } else {
      if (Array.isArray(value.fields))
        facts.push(["fields", String(value.fields.length)])
      if (node.kind === "event" && typeof value.schemaVersion === "number")
        facts.push(["schema", `v${value.schemaVersion}`])
      if (node.caption) facts.push(["role", node.caption])
    }
    const card: DomainCard = {
      id: node.id,
      kind: node.kind,
      label: node.label,
      facts,
      action: navigates
        ? "Explore aggregate"
        : node.kind === "context"
          ? "Inspect declarations"
          : Array.isArray(value.fields)
            ? "Inspect fields"
            : "Inspect definition",
      navigates,
      x,
      y,
      width: cardWidth,
      height: cardHeight,
    }
    cardsById.set(card.id, card)
    return card
  }

  function grid(count: number, maxColumns: number): Grid {
    const columns = Math.min(Math.max(count, 1), maxColumns)
    return {
      columns,
      width: columns * cardWidth + (columns - 1) * columnGap,
      height: count
        ? Math.ceil(count / columns) * (cardHeight + rowGap) - rowGap
        : 0,
    }
  }

  function placeGrid(
    nodes: DomainNode[],
    shape: Grid,
    x: number,
    y: number,
    heading?: { id: string; label: string }
  ) {
    if (heading) {
      headings.push({ ...heading, x, y, width: shape.width })
      y += headingSpace
    }
    return nodes.map((node, i) => {
      const card = place(
        node,
        x + (i % shape.columns) * (cardWidth + columnGap),
        y + Math.floor(i / shape.columns) * (cardHeight + rowGap)
      )
      rails.set(card.id, [x - gutter, x + shape.width + gutter])
      return card
    })
  }

  const targetNodes = (members: DomainLink[]) =>
    members.map(({ target }) => model.nodes.get(target)!)

  function connect(
    owner: DomainCard,
    cards: DomainCard[],
    members: DomainLink[],
    outerRails?: [left: number, right: number]
  ) {
    if (!cards.length) return
    const trunkY = bottom(owner) + 26
    const columns = [...new Set(cards.map((card) => card.x))].sort(
      (a, b) => a - b
    )
    cards.forEach((card, i) => {
      // One spine between each facing pair of columns: short, visible branches
      // arrive at the side of each card without running along an entire row.
      const col = columns.indexOf(card.x)
      const facesRight = outerRails
        ? col % 2 === 1
        : col % 2 === 0 && col + 1 < columns.length
      const end = facesRight ? right(card) : card.x
      const rail = outerRails
        ? outerRails[Number(facesRight)]
        : end +
          (facesRight ? 1 : -1) *
            (columns.length > 1 ? columnGap / 2 : railMargin)
      const midY = card.y + card.height / 2
      edges.push({
        ...members[i],
        path: line(
          [center(owner), bottom(owner)],
          [center(owner), trunkY],
          [rail, trunkY],
          [rail, midY],
          [end, midY]
        ),
      })
    })
  }

  let references = links.filter(({ kind }) => kind === "field-reference")
  if (aggregate) {
    const members = links.filter(
      (link) => link.source === aggregate.id && link.kind !== "field-reference"
    )
    const groups = [
      {
        id: "entities",
        label: "Entities",
        members: members
          .filter(
            ({ kind }) => kind === "root-entity" || kind === "owns-entity"
          )
          .sort(
            (a, b) =>
              Number(b.kind === "root-entity") -
              Number(a.kind === "root-entity")
          ),
      },
      {
        id: "events",
        label: "Events",
        members: members.filter(({ kind }) => kind === "declares-event"),
      },
    ]
      .filter((group) => group.members.length)
      .map((group) => ({
        ...group,
        shape: grid(
          group.members.length,
          compact
            ? 2
            : group.id === "entities" && group.members.length <= 4
              ? 1
              : group.members.length >= 8
                ? 4
                : 2
        ),
      }))
    const family = new Set([
      aggregate.id,
      ...members.map(({ target }) => target),
    ])
    // Expand only explicit targets of this family, not their descendants or
    // unrelated incoming references from other families.
    references = references.filter(({ source }) => family.has(source))
    const external = [...new Set(references.map(({ target }) => target))]
      .filter((id) => !family.has(id))
      .sort(compare)
      .map((id) => model.nodes.get(id)!)
    const externalGrid = grid(external.length, compact ? 2 : 3)
    const groupWidth = compact
      ? Math.max(0, ...groups.map(({ shape }) => shape.width))
      : groups.reduce(
          (total, { shape }, i) => total + shape.width + (i ? blockGap : 0),
          0
        )
    const width = Math.max(
      ownerWidth,
      groupWidth,
      external.length ? externalGrid.width : 0
    )
    const owner = place(aggregate, (width - ownerWidth) / 2, 0)
    let x = (width - groupWidth) / 2
    let y = bottom(owner) + 64
    let groupBottom = bottom(owner)
    for (const group of groups) {
      const cards = placeGrid(
        targetNodes(group.members),
        group.shape,
        compact ? (width - group.shape.width) / 2 : x,
        y,
        {
          id: `heading:${aggregate.id}:${group.id}`,
          label: group.label,
        }
      )
      connect(
        owner,
        cards,
        group.members,
        compact
          ? [
              (width - groupWidth) / 2 - railMargin,
              (width + groupWidth) / 2 + railMargin,
            ]
          : undefined
      )
      groupBottom = Math.max(groupBottom, y + headingSpace + group.shape.height)
      if (compact) y = groupBottom + 38
      else x += group.shape.width + blockGap
    }
    if (external.length)
      placeGrid(
        external,
        externalGrid,
        (width - externalGrid.width) / 2,
        groupBottom + 46,
        {
          id: `heading:${aggregate.id}:references`,
          label: "References",
        }
      )
  } else {
    const nodes = [...model.nodes.values()].sort((a, b) => compare(a.id, b.id))
    const memberships = links.filter(({ kind }) => kind === "aggregate-context")
    const assigned = new Set(memberships.map(({ target }) => target))
    const unassigned = nodes.filter(
      ({ kind, id }) => kind === "aggregate" && !assigned.has(id)
    )
    const maxColumns = compact ? 2 : 3
    const clusters = nodes
      .filter(({ kind }) => kind === "context")
      .map((context) => {
        const children = memberships.filter(
          ({ source }) => source === context.id
        )
        const shape = grid(children.length, maxColumns)
        return {
          context,
          children,
          shape,
          width: Math.max(ownerWidth, shape.width),
        }
      })
    const orphanGrid = grid(unassigned.length, maxColumns)
    const clusterWidth = compact
      ? Math.max(0, ...clusters.map(({ width }) => width))
      : clusters.reduce(
          (total, { width }, i) => total + width + (i ? blockGap : 0),
          0
        )
    const width = Math.max(
      clusterWidth,
      unassigned.length ? orphanGrid.width : 0
    )
    let x = (width - clusterWidth) / 2
    let y = 0
    let clusterBottom = 0
    for (const { context, children, shape, width: blockWidth } of clusters) {
      const centerX = compact ? width / 2 : x + blockWidth / 2
      const owner = place(context, centerX - ownerWidth / 2, y)
      const gridY = bottom(owner) + 40
      const cards = placeGrid(
        targetNodes(children),
        shape,
        centerX - shape.width / 2,
        gridY
      )
      connect(owner, cards, children)
      clusterBottom = Math.max(
        clusterBottom,
        children.length ? gridY + shape.height : bottom(owner)
      )
      if (compact) y = clusterBottom + blockGap
      else x += blockWidth + blockGap
    }
    if (unassigned.length)
      placeGrid(
        unassigned,
        orphanGrid,
        (width - orphanGrid.width) / 2,
        clusters.length ? clusterBottom + blockGap : 0,
        {
          id: "heading:other-aggregates",
          label: "Aggregates",
        }
      )
  }

  const cards = [...cardsById.values()]
  if (!cards.length)
    return {
      cards,
      edges,
      headings,
      bounds: { x: 0, y: 0, width: 1, height: 1 },
    }

  const obstacles: Rect[] = [
    ...cards,
    ...headings.map((heading) => ({ ...heading, height: headingHeight })),
  ]
  const box = obstacles.reduce(
    (box, rect) => ({
      left: Math.min(box.left, rect.x),
      right: Math.max(box.right, right(rect)),
      top: Math.min(box.top, rect.y),
      bottom: Math.max(box.bottom, bottom(rect)),
    }),
    { left: Infinity, right: -Infinity, top: Infinity, bottom: -Infinity }
  )

  // Reserve the left membership rails in the bounds before considering refs.
  let left = box.left - (edges.length ? Math.max(railMargin, columnGap / 2) : 0)
  let rightmost = box.right + (compact && edges.length ? railMargin : 0)
  let lowest = box.bottom
  for (const reference of references) {
    const source = cardsById.get(reference.source)
    const target = cardsById.get(reference.target)
    if (!source || !target) continue
    const sx = center(source) - 10
    const tx = center(target) + 10
    const sy = bottom(source) + gutter
    const ty = bottom(target) + gutter
    const candidates = [
      ...(rails.get(source.id) ?? []),
      ...(rails.get(target.id) ?? []),
      box.left - gutter,
      box.right + gutter,
    ]
    candidates.sort(
      (a, b) =>
        Math.abs(a - sx) +
        Math.abs(a - tx) -
        Math.abs(b - sx) -
        Math.abs(b - tx)
    )
    // All cards on a band share its row pitch, so horizontal reference
    // segments fit below a row. Choose the nearest unblocked vertical rail.
    const rail = candidates.find((x) =>
      obstacles.every(
        (rect) =>
          x <= rect.x ||
          x >= right(rect) ||
          Math.max(sy, ty) <= rect.y ||
          Math.min(sy, ty) >= bottom(rect)
      )
    )!
    edges.push({
      ...reference,
      path:
        sy === ty
          ? line([sx, bottom(source)], [sx, sy], [tx, ty], [tx, bottom(target)])
          : line(
              [sx, bottom(source)],
              [sx, sy],
              [rail, sy],
              [rail, ty],
              [tx, ty],
              [tx, bottom(target)]
            ),
    })
    if (sy !== ty) {
      left = Math.min(left, rail)
      rightmost = Math.max(rightmost, rail)
    }
    lowest = Math.max(lowest, sy, ty)
  }
  return {
    cards,
    edges,
    headings,
    bounds: {
      x: left,
      y: box.top,
      width: rightmost - left,
      height: lowest - box.top,
    },
  }
}
