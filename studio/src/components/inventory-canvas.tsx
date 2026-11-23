import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react"
import { useTheme } from "@rostfrei/ui/theme"
import {
  Background,
  BackgroundVariant,
  BaseEdge,
  Handle,
  Position,
  ReactFlow,
  getBezierPath,
  getViewportForBounds,
  useReactFlow,
  useStore,
  useViewport,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
  type Rect,
} from "@xyflow/react"
import {
  ArrowLeft,
  ArrowRight,
  Box,
  Database,
  Focus,
  GitBranch,
  Layers,
  Minus,
  Plus,
  Server,
  X,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import "./inventory-canvas.css"

const kinds = {
  source: { label: "Broker source", icon: Server },
  stream: { label: "Stream", icon: Layers },
  consumer: { label: "Consumer", icon: GitBranch },
  bucket: { label: "KV bucket", icon: Database },
  server: { label: "NATS node", icon: Server },
  monitoring: { label: "Server monitoring", icon: Server },
  model: { label: "Read model", icon: Box },
  store: { label: "Declared KV store", icon: Database },
}

export type Resource = {
  id: string
  kind: keyof typeof kinds
  title: string
  caption?: string
  summary?: string
  warning?: boolean
  drill?: boolean
  details: ReactNode
}

type Relationship =
  | "reports-stream"
  | "reports-consumer"
  | "reports-bucket"
  | "backs-bucket"
  | "declares-store"
export type ResourceLink = {
  source: string
  target: string
  kind: Relationship
}
const storeLink = (kind?: Relationship) =>
  kind === "backs-bucket" || kind === "declares-store"

type Card = Node<
  {
    resource: Resource
    selected: boolean
    activate: (resource: Resource, trigger: HTMLButtonElement) => void
    narrow: boolean
  },
  "resource"
>
type Wire = Edge<{ kind: Relationship }, "resource">
const nodeTypes = { resource: ResourceCard }
const edgeTypes = { resource: ResourceWire }
const cardWidth = 242
const cardHeight = 114
const row = 140
const column = 302
const narrowQuery = "(width < 620px)"
const narrowLayout = () => window.matchMedia(narrowQuery).matches
function subscribeNarrow(callback: () => void) {
  const media = window.matchMedia(narrowQuery)
  media.addEventListener("change", callback)
  return () => media.removeEventListener("change", callback)
}

export function InventoryCanvas({
  label,
  columns,
  links = [],
  scope,
  back,
  drill,
  title,
  actions,
  notices,
}: {
  label: string
  columns: Resource[][]
  links?: ResourceLink[]
  scope?: string
  back?: { label: string; action: () => void }
  drill?: (id: string) => void
  title: string
  actions: ReactNode
  notices?: ReactNode
}) {
  const { resolvedTheme } = useTheme()
  const trigger = useRef<HTMLButtonElement>(null)
  const close = useRef<HTMLButtonElement>(null)
  const [selected, select] = useState<string>()
  const narrow = useSyncExternalStore(subscribeNarrow, narrowLayout)
  const inspecting = columns.flat().find(({ id }) => id === selected)
  if (selected && !inspecting) select(undefined)

  const inspectedId = inspecting?.id
  useEffect(() => {
    if (inspectedId) close.current?.focus({ preventScroll: true })
  }, [inspectedId])

  function dismiss(restoreFocus = true) {
    select(undefined)
    if (restoreFocus) trigger.current?.focus({ preventScroll: true })
  }

  const nodes = columns.flatMap((cards, index) => {
    const offset = columns
      .slice(0, index)
      .reduce((total, cards) => total + cards.length * row, 0)
    return cards.map((resource, item): Card => ({
      id: resource.id,
      type: "resource",
      position: narrow
        ? { x: 0, y: offset + item * row }
        : { x: index * column, y: item * row },
      width: cardWidth,
      height: cardHeight,
      data: {
        resource,
        selected: resource.id === inspectedId,
        narrow,
        activate(resource, button) {
          if (resource.drill) {
            dismiss(false)
            drill?.(resource.id)
            return
          }
          trigger.current = button
          select(resource.id)
        },
      },
    }))
  })
  const bounds = nodes.reduce<Rect>(
    (area, node) => ({
      x: 0,
      y: 0,
      width: Math.max(area.width, node.position.x + cardWidth),
      height: Math.max(area.height, node.position.y + cardHeight),
    }),
    { x: 0, y: 0, width: 0, height: 0 }
  )
  const edges = links.map(({ source, target, kind }): Wire => ({
    id: JSON.stringify([kind, source, target]),
    type: "resource",
    source,
    target,
    data: { kind },
  }))

  return (
    <section
      aria-label={label}
      className="inventory-canvas"
      onKeyDown={(event) => {
        if (event.key === "Escape" && inspecting) {
          event.stopPropagation()
          dismiss()
        }
      }}
    >
      <ReactFlow<Card, Wire>
        key={scope ?? "overview"}
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        colorMode={resolvedTheme}
        minZoom={0.18}
        maxZoom={1.75}
        nodesDraggable={false}
        nodesConnectable={false}
        nodesFocusable={false}
        edgesFocusable={false}
        elementsSelectable={false}
        panOnScroll
        zoomOnScroll={false}
        zoomOnDoubleClick={false}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={20}
          size={1}
          color="var(--canvas-dot)"
        />
        <ResourceCamera bounds={bounds} />
      </ReactFlow>
      <div className="inventory-floating">
        <header className="inventory-actions" aria-label={`${title} actions`}>
          {back && (
            <Button
              variant="ghost"
              size="icon"
              title={back.label}
              aria-label={back.label}
              onClick={() => {
                dismiss(false)
                back.action()
              }}
            >
              <ArrowLeft />
            </Button>
          )}
          <h1>{title}</h1>
          {actions}
        </header>
        <div className="inventory-notices">{notices}</div>
      </div>
      {links.length > 0 && (
        <div className="inventory-edge-legend" aria-label="Resource link types">
          {links.some(({ kind }) => !storeLink(kind)) && (
            <span>
              <i /> Reported by
            </span>
          )}
          {links.some(({ kind }) => kind === "backs-bucket") && (
            <span>
              <i data-storage /> Backing stream
            </span>
          )}
          {links.some(({ kind }) => kind === "declares-store") && (
            <span>
              <i data-storage /> Declared storage
            </span>
          )}
        </div>
      )}
      {inspecting && (
        <aside className="inventory-overlay" aria-label="Inventory details">
          <header>
            <div>
              <p>{kinds[inspecting.kind].label}</p>
              <h2>{inspecting.title}</h2>
            </div>
            <Button
              ref={close}
              variant="ghost"
              size="icon"
              aria-label="Close details"
              onClick={() => dismiss()}
            >
              <X />
            </Button>
          </header>
          <div className="inventory-overlay-body">{inspecting.details}</div>
        </aside>
      )}
    </section>
  )
}

function ResourceCard({
  data: { resource, selected, narrow, activate },
}: NodeProps<Card>) {
  const { label, icon: Icon } = kinds[resource.kind]
  return (
    <div className="inventory-card-frame" data-graph-node>
      <Handle type="target" position={narrow ? Position.Top : Position.Left} />
      <Handle
        type="source"
        position={narrow ? Position.Bottom : Position.Right}
      />
      <button
        type="button"
        className="inventory-card nopan nodrag"
        data-inventory-resource={resource.kind}
        data-inventory-stream={
          resource.kind === "stream" ? resource.title : undefined
        }
        aria-label={`${resource.drill ? "Explore" : "Inspect"} ${label.toLowerCase()} ${resource.title}`}
        aria-pressed={resource.drill ? undefined : selected}
        onClick={(event) => activate(resource, event.currentTarget)}
      >
        <span className="inventory-card-kind">
          <Icon size={14} /> {label}
          {resource.drill && (
            <ArrowRight className="inventory-drill" size={14} />
          )}
        </span>
        <strong title={resource.title}>{resource.title}</strong>
        {resource.caption && (
          <span className="inventory-card-caption" title={resource.caption}>
            {resource.caption}
          </span>
        )}
        {resource.summary && (
          <span
            className="inventory-card-summary"
            data-warning={resource.warning}
            title={resource.summary}
          >
            {resource.summary}
          </span>
        )}
      </button>
    </div>
  )
}

function ResourceWire({
  id,
  source,
  target,
  data,
  ...positions
}: EdgeProps<Wire>) {
  const [path] = getBezierPath(positions)
  // Route vertically stacked cards through a side gutter, not through other cards.
  const vertical = positions.sourcePosition === Position.Bottom
  const { sourceX, sourceY, targetX, targetY } = positions
  const gutter = Math.max(sourceX, targetX) + 142
  const verticalPath = `M${sourceX},${sourceY} v9 H${gutter} V${targetY - 9} H${targetX} V${targetY}`
  // A bucket can be reported without its backing stream in the sample. Route
  // that source-to-bucket link above the intervening column of stream cards.
  const bypassesColumn = !vertical && targetX - sourceX > column
  const columnBypass = `M${sourceX},${sourceY} h14 V-16 H${targetX - 14} V${targetY} H${targetX}`
  return (
    <g
      data-graph-edge
      data-inventory-edge={data?.kind}
      data-source-id={source}
      data-target-id={target}
    >
      <BaseEdge
        id={id}
        path={
          bypassesColumn
            ? columnBypass
            : vertical && targetY - sourceY !== row - cardHeight
              ? verticalPath
              : path
        }
        interactionWidth={0}
        className={
          storeLink(data?.kind)
            ? "inventory-storage-wire"
            : "inventory-membership-wire"
        }
      />
    </g>
  )
}

function ResourceCamera({ bounds }: { bounds: Rect }) {
  const { setViewport, zoomIn, zoomOut, viewportInitialized } = useReactFlow()
  const width = useStore((state) => state.width)
  const height = useStore((state) => state.height)
  const { zoom } = useViewport()
  const { width: graphWidth, height: graphHeight } = bounds
  const area = useMemo(
    () => ({ x: 0, y: 0, width: graphWidth, height: graphHeight }),
    [graphWidth, graphHeight]
  )

  useEffect(() => {
    if (!viewportInitialized || !area.width || !width || !height) return
    // Start at the readable beginning of long inventories, rather than shrinking
    // every resource to fit. The camera controls can reveal the full extent.
    void setViewport({ x: width < 620 ? 20 : 30, y: 110, zoom: 1 })
  }, [viewportInitialized, area, width, height, setViewport])

  function fit() {
    if (!area.width) return
    const viewport = getViewportForBounds(
      area,
      width,
      Math.max(100, height - 150),
      0.18,
      1,
      0.1
    )
    void setViewport({ ...viewport, y: viewport.y + 104 })
  }

  return (
    <div
      className="inventory-camera nopan nodrag nowheel"
      aria-label="Canvas controls"
      role="group"
    >
      <Button
        variant="ghost"
        size="icon"
        aria-label="Zoom out"
        onClick={() => void zoomOut()}
      >
        <Minus />
      </Button>
      <span>{Math.round(zoom * 100)}%</span>
      <Button
        variant="ghost"
        size="icon"
        aria-label="Zoom in"
        onClick={() => void zoomIn()}
      >
        <Plus />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label="Fit resources to view"
        disabled={!area.width}
        onClick={fit}
      >
        <Focus />
      </Button>
    </div>
  )
}
