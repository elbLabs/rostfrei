import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { useTheme } from "@rostfrei/ui/theme"
import { Box, Component, GitBranch, Layers } from "lucide-react"
import {
  BaseEdge,
  Handle,
  Panel,
  Position,
  ReactFlow,
  ViewportPortal,
  getViewportForBounds,
  useReactFlow,
  useStore,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
  type Rect,
  type ReactFlowInstance,
} from "@xyflow/react"
import { GraphBackground, GraphControls, GraphHeading } from "./graph-tools"
import { GraphCard } from "./graph-card"
import {
  domainKindLabels,
  type DomainCard,
  type DomainEdge,
  type scopeDomain,
} from "./domain-layout"

type CanvasNode = Node<
  {
    card: DomainCard
    selected: boolean
    select: (card: DomainCard) => void
  },
  "domain"
>
type CanvasEdge = Edge<
  { link: DomainEdge; highlighted: boolean; muted: boolean },
  "domain"
>
const icons = {
  context: Layers,
  aggregate: Box,
  entity: Component,
  event: GitBranch,
}
const nodeTypes = { domain: CardNode }
const edgeTypes = { domain: RelationshipEdge }

export function DomainCanvas({
  layout,
  title,
  leading,
  selected,
  select,
  onPaneClick,
  tools,
  children,
}: {
  layout: ReturnType<typeof scopeDomain>
  title: string
  leading: ReactNode
  selected?: string
  select: (card: DomainCard) => void
  onPaneClick?: () => void
  tools: ReactNode
  children: ReactNode
}) {
  const { resolvedTheme } = useTheme()
  const [hover, setHover] = useState<string>()
  const [focus, setFocus] = useState<string>()
  const flow = useRef<ReactFlowInstance<CanvasNode, CanvasEdge> | null>(null)
  const tabbing = useRef(false)
  const active = selected ?? hover ?? focus
  const nodes = useMemo<CanvasNode[]>(
    () =>
      layout.cards.map((card) => ({
        id: card.id,
        type: "domain",
        position: { x: card.x, y: card.y },
        width: card.width,
        height: card.height,
        data: { card, selected: selected === card.id, select },
      })),
    [layout, selected, select]
  )
  const edges = useMemo<CanvasEdge[]>(
    () =>
      layout.edges.map((link) => {
        const highlighted = active === link.source || active === link.target
        return {
          id: JSON.stringify([link.kind, link.source, link.target]),
          type: "domain",
          source: link.source,
          target: link.target,
          zIndex: highlighted ? 1 : 0,
          data: {
            link,
            highlighted,
            muted: !!active && !highlighted,
          },
        }
      }),
    [layout, active]
  )
  return (
    <section aria-label="Domain canvas" className="domain-canvas-area">
      <div
        className="message-graph domain-canvas-stage"
        onKeyDownCapture={(event) => {
          tabbing.current = event.key === "Tab"
        }}
        onPointerDownCapture={() => {
          tabbing.current = false
        }}
        onFocusCapture={(event) => {
          if (event.target.matches(":focus-visible")) setHover(undefined)
          const card = event.target.closest<HTMLElement>("[data-domain-node]")
          setFocus(card?.dataset.domainNode)
          // Tab can enter the map from its toolbar or another card. Preserve the
          // user's zoom and don't recenter when closing an inspector restores focus.
          if (tabbing.current && card && flow.current) {
            const stage = event.currentTarget
            requestAnimationFrame(() => {
              if (card === document.activeElement && flow.current)
                revealCard(card, stage, flow.current)
            })
          }
          tabbing.current = false
        }}
        onBlurCapture={() => setFocus(undefined)}
      >
        <ReactFlow<CanvasNode, CanvasEdge>
          proOptions={{ hideAttribution: true }}
          onInit={(instance) => {
            flow.current = instance
          }}
          colorMode={resolvedTheme}
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          minZoom={0.05}
          maxZoom={2}
          nodesConnectable={false}
          nodesDraggable={false}
          nodesFocusable={false}
          edgesFocusable={false}
          elementsSelectable={false}
          panOnDrag
          zoomOnScroll
          zoomOnPinch
          autoPanOnNodeFocus={false}
          onNodeMouseEnter={(_, node) => setHover(node.id)}
          onNodeMouseLeave={() => setHover(undefined)}
          onPaneClick={onPaneClick}
          zoomOnDoubleClick={false}
          aria-label="Domain navigation graph"
        >
          <Panel
            position="top-left"
            className="domain-floating-tools graph-chrome"
            aria-label="Domain tools"
          >
            <GraphHeading title={title} icon={leading}>
              {tools}
              <CanvasControls bounds={layout.bounds} />
            </GraphHeading>
          </Panel>
          <GraphBackground />
          <InitialViewport bounds={layout.bounds} />
          <ViewportPortal>
            {layout.headings.map(({ id, label, x, y, width }) => (
              <div
                key={id}
                className="domain-group-heading"
                style={{ left: x, top: y, width }}
              >
                {label}
              </div>
            ))}
          </ViewportPortal>
        </ReactFlow>
        {children}
      </div>
      <footer className="flow-legend domain-canvas-legend">
        {layout.edges.some(
          ({ kind }) =>
            kind === "root-entity" ||
            kind === "owns-entity" ||
            kind === "declares-event"
        ) && (
          <span>
            <i className="domain-membership-key" /> Aggregate membership
          </span>
        )}
        {layout.edges.some(({ kind }) => kind === "aggregate-context") && (
          <span>
            <i className="domain-membership-key" /> Context membership
          </span>
        )}
        {layout.edges.some(({ kind }) => kind === "field-reference") && (
          <span>
            <i /> Field reference
          </span>
        )}
      </footer>
    </section>
  )
}

function useCanvasSize() {
  const width = useStore((state) => state.width)
  const height = useStore((state) => state.height)
  return { width, height }
}

function revealCard(
  card: HTMLElement,
  stage: HTMLElement,
  flow: ReactFlowInstance<CanvasNode, CanvasEdge>
) {
  const rect = card.getBoundingClientRect()
  const bounds = stage.getBoundingClientRect()
  const toolbar = stage
    .querySelector(".domain-floating-tools")
    ?.getBoundingClientRect()
  const top =
    (toolbar && rect.left <= toolbar.right ? toolbar.bottom : bounds.top) + 12
  const dx =
    Math.max(0, bounds.left + 12 - rect.left) +
    Math.min(0, bounds.right - 12 - rect.right)
  const dy =
    Math.max(0, top - rect.top) + Math.min(0, bounds.bottom - 12 - rect.bottom)
  if (dx || dy) {
    const { x, y, zoom } = flow.getViewport()
    void flow.setViewport({ x: x + dx, y: y + dy, zoom })
  }
}

function domainViewport(
  bounds: Rect,
  width: number,
  height: number,
  minZoom: number
) {
  const top = 56
  const viewport = getViewportForBounds(
    bounds,
    width,
    Math.max(1, height - top),
    minZoom,
    1,
    0.1
  )
  // A large family opens at its owner, at a readable scale. Fit is always available.
  const y = Math.max(top + 12 - bounds.y * viewport.zoom, viewport.y + top)
  return { ...viewport, y }
}

function InitialViewport({ bounds }: { bounds: Rect }) {
  const { width, height } = useCanvasSize()
  const { setViewport, viewportInitialized } = useReactFlow()
  useEffect(() => {
    if (!viewportInitialized || width <= 0 || height <= 0) return
    void setViewport(domainViewport(bounds, width, height, 0.85))
  }, [bounds, width, height, setViewport, viewportInitialized])
  return null
}

function CardNode({ data: { card, selected, select } }: NodeProps<CanvasNode>) {
  const { id, label, kind, facts, action, navigates } = card
  const Icon = icons[kind]
  const kindLabel = domainKindLabels[kind]
  return (
    <div className="domain-card-frame" data-graph-node data-domain-node-id={id}>
      <Handle
        type="target"
        position={Position.Left}
        className="domain-handle"
      />
      <Handle
        type="source"
        position={Position.Right}
        className="domain-handle"
      />
      <GraphCard
        className="domain-card"
        kind={kindLabel}
        icon={<Icon size={13} aria-hidden="true" />}
        label={label}
        facts={facts}
        footer={action}
        accessibleLabel={
          navigates
            ? `Explore ${kindLabel} ${label}`
            : `${action}: ${kindLabel} ${label}`
        }
        selected={navigates ? undefined : selected}
        title={label}
        onClick={() => select(card)}
        attributes={{ "data-domain-node": id, "data-domain-kind": kind }}
      />
    </div>
  )
}

function RelationshipEdge({ id, source, target, data }: EdgeProps<CanvasEdge>) {
  const portId = `port-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`
  if (!data) return null
  const { link, highlighted, muted } = data
  return (
    <g
      data-graph-edge
      data-domain-edge={link.kind}
      data-edge-relationship={link.kind}
      data-source-id={source}
      data-target-id={target}
      data-edge-traced={highlighted || undefined}
      data-edge-muted={muted || undefined}
      className="domain-relationship"
    >
      <defs>
        <marker
          id={portId}
          markerUnits="userSpaceOnUse"
          markerWidth="9"
          markerHeight="9"
          viewBox="-4.5 -4.5 9 9"
          refX="0"
          refY="0"
        >
          <circle r="3.2" fill="currentColor" />
        </marker>
      </defs>
      <BaseEdge
        id={id}
        path={link.path}
        className="domain-edge"
        interactionWidth={0}
        vectorEffect="non-scaling-stroke"
        markerEnd={`url(#${portId})`}
        style={{
          stroke: "currentColor",
          strokeWidth: highlighted ? 2.8 : 1.65,
          strokeDasharray: link.kind === "field-reference" ? "6 4" : undefined,
        }}
      />
    </g>
  )
}

function CanvasControls({ bounds }: { bounds: Rect }) {
  const { setViewport } = useReactFlow()
  const { width, height } = useCanvasSize()
  return (
    <GraphControls
      fitLabel="Fit domain to view"
      onFit={() =>
        void setViewport(domainViewport(bounds, width, height, 0.05))
      }
    />
  )
}
