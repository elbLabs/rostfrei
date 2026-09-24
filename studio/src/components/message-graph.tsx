import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react"
import { createPortal } from "react-dom"
import {
  ArrowRight,
  CircleDot,
  Focus,
  GitBranch,
  Minus,
  PanelRight,
  Plus,
  Radio,
  Workflow,
} from "lucide-react"
import {
  Background,
  Handle,
  Position,
  ReactFlow,
  getBezierPath,
  useReactFlow,
  useViewport,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
  type ReactFlowInstance,
} from "@xyflow/react"

import { MessageInspector } from "@/components/message-inspector"
import { Button } from "@/components/ui/button"
import {
  layoutMessageGraph,
  FIXTURE_WIDTH,
  MESSAGE_HEIGHT,
  MESSAGE_WIDTH,
} from "@/lib/graph"
import {
  kindLabels,
  messageFacts,
  messageState,
} from "@/lib/message-presentation"
import type {
  FlowView,
  MessageGraphNode,
  StudioLayout,
  OperationMessageSeries,
} from "@/lib/types"
import { useMediaQuery } from "@/lib/use-media-query"
import { cn } from "@/lib/utils"

interface MessageGraphProps {
  nodes: MessageGraphNode[]
  view: FlowView
  layoutMode: StudioLayout
  emptyMessage?: string
  capture?: OperationMessageSeries["capture"]
}

type MessageFlowNode = Node<
  {
    message: MessageGraphNode
    view: FlowView
    selected: boolean
    onSelect: (id: string) => void
  },
  "message"
>
type MessageFlowEdge = Edge<
  {
    relationship: "causation" | "stream-order" | "context"
  },
  "message"
>

const nodeTypes = { message: MessageNode }
const edgeTypes = { message: MessageEdge }
const kindIcons = {
  command: CircleDot,
  "domain-event": GitBranch,
  "integration-event": Radio,
}

export function MessageGraph({
  nodes,
  view,
  layoutMode,
  emptyMessage,
  capture,
}: MessageGraphProps) {
  const compact = useMediaQuery("(max-width: 1100px)")
  const canvas = layoutMode === "canvas"
  const [selectedId, setSelectedId] = useState<string>()
  const [detailsOpen, setDetailsOpen] = useState(false)
  const [canvasInspectorOpen, setCanvasInspectorOpen] = useState(false)
  const inspectorVisible = compact
    ? detailsOpen
    : !canvas || canvasInspectorOpen
  const [controlsRoot, setControlsRoot] = useState<HTMLDivElement | null>(null)
  const viewportRef = useRef<HTMLDivElement>(null)
  const mobileTrigger = useRef<HTMLElement | null>(null)
  const [flow, setFlow] = useState<ReactFlowInstance<
    MessageFlowNode,
    MessageFlowEdge
  > | null>(null)
  const layout = useMemo(() => layoutMessageGraph(nodes), [nodes])
  const selected =
    nodes.find((node) => node.id === selectedId) ??
    nodes.find((node) => node.subject === true) ??
    nodes.find(
      (node) =>
        node.kind === "command" && !node.context && node.subject !== false
    ) ??
    nodes[0]
  const highlightedId = inspectorVisible ? selected?.id : undefined
  const select = useCallback(
    (id: string) => {
      mobileTrigger.current =
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null
      setSelectedId(id)
      setDetailsOpen(true)
      if (canvas) setCanvasInspectorOpen(true)
    },
    [canvas]
  )
  const flowNodes = useMemo<MessageFlowNode[]>(
    () =>
      layout.nodes.map((node) => ({
        id: node.id,
        type: "message",
        position: { x: node.x, y: node.y },
        width: node.context ? FIXTURE_WIDTH : MESSAGE_WIDTH,
        height: MESSAGE_HEIGHT,
        style: {
          width: node.context ? FIXTURE_WIDTH : MESSAGE_WIDTH,
          height: MESSAGE_HEIGHT,
        },
        data: {
          message: node,
          view,
          selected: node.id === highlightedId,
          onSelect: select,
        },
        draggable: false,
        selectable: false,
        focusable: false,
      })),
    [layout.nodes, view, highlightedId, select]
  )
  const flowEdges = useMemo<MessageFlowEdge[]>(
    () =>
      layout.edges.map((edge) => ({
        id: edge.id,
        type: "message",
        source: edge.source.id,
        target: edge.target.id,
        data: { relationship: edge.relationship },
        focusable: false,
        selectable: false,
      })),
    [layout.edges]
  )
  const nodeIds = nodes.map((node) => node.id).join("\u0000")
  const fit = useCallback(() => {
    if (view !== "running")
      void flow?.fitView({
        padding: 0.04,
        minZoom: 0.2,
        maxZoom: 1,
        duration: 0,
      })
  }, [flow, view])

  useEffect(() => {
    const frame = window.requestAnimationFrame(fit)
    return () => window.cancelAnimationFrame(frame)
  }, [fit, nodeIds, compact])

  useEffect(() => {
    const element = viewportRef.current
    if (!element || compact) return
    let frame = 0
    const observer = new ResizeObserver(() => {
      window.cancelAnimationFrame(frame)
      frame = window.requestAnimationFrame(fit)
    })
    observer.observe(element)
    return () => {
      observer.disconnect()
      window.cancelAnimationFrame(frame)
    }
  }, [fit, compact])

  useEffect(() => {
    if (compact && detailsOpen)
      viewportRef.current
        ?.closest(".graph-workspace")
        ?.querySelector<HTMLElement>(".message-inspector")
        ?.focus()
  }, [compact, detailsOpen])

  const fixtureCount = nodes.filter((node) => node.context).length
  const uncertain = nodes.some(
    (node) => !node.context && node.edgeFidelity === "grouped"
  )
  const title =
    view === "expected"
      ? "Expected flow"
      : view === "running"
        ? "Execution in progress"
        : view === "unavailable"
          ? "Incomplete execution"
          : view === "predicted"
            ? "Preview flow"
            : "Observed flow"
  const dismissInspector = () => {
    setDetailsOpen(false)
    setCanvasInspectorOpen(false)
    window.requestAnimationFrame(() => mobileTrigger.current?.focus())
  }

  return (
    <div
      className={cn(
        "graph-workspace",
        !compact && !inspectorVisible && "inspector-collapsed",
        compact && detailsOpen && "mobile-inspecting"
      )}
    >
      <section className="flow-panel" aria-label={title}>
        <header className="flow-heading">
          <div>
            <Workflow size={16} />
            <h2>{title}</h2>
          </div>
          <span>
            {nodes.length - fixtureCount}{" "}
            {nodes.length - fixtureCount === 1 ? "message" : "messages"} ·{" "}
            {fixtureCount} fixture {fixtureCount === 1 ? "event" : "events"}
          </span>
          {canvas && !compact && (
            <Button
              className="inspector-toggle"
              variant="ghost"
              size="sm"
              disabled={!nodes.length}
              aria-expanded={inspectorVisible}
              aria-label={
                inspectorVisible
                  ? "Close message details"
                  : "Open message details"
              }
              onClick={(event) => {
                if (inspectorVisible) dismissInspector()
                else {
                  mobileTrigger.current = event.currentTarget
                  setCanvasInspectorOpen(true)
                }
              }}
            >
              <PanelRight /> Details
            </Button>
          )}
        </header>
        <div className="message-graph" ref={viewportRef}>
          {nodes.length === 0 ? (
            <div className="graph-empty">
              <Workflow />
              <h3>No flow to display</h3>
              <p>
                {emptyMessage ??
                  "Select a test to explore its expected messages."}
              </p>
            </div>
          ) : compact ? (
            <ol className="message-list" aria-label="Messages in this flow">
              {nodes.map((node) => {
                const parent = nodes.find(
                  (candidate) => candidate.id === node.parentId
                )
                return (
                  <li key={node.id}>
                    <p className="list-relationship">
                      {node.context
                        ? "Given · fixture state"
                        : node.edgeRelationship === "context"
                          ? "When · root command"
                          : node.subject === true
                            ? "Root command"
                            : parent && node.edgeFidelity === "exact"
                              ? `${view === "expected" ? "Expected after" : "Caused by"} ${parent.name}`
                              : "Associated message · no resolved cause"}
                    </p>
                    <MessageCard
                      node={node}
                      view={view}
                      selected={highlightedId === node.id}
                      onSelect={select}
                    />
                  </li>
                )
              })}
            </ol>
          ) : (
            <ReactFlow<MessageFlowNode, MessageFlowEdge>
              className="message-flow"
              nodes={flowNodes}
              edges={flowEdges}
              nodeTypes={nodeTypes}
              edgeTypes={edgeTypes}
              minZoom={0.2}
              maxZoom={1.5}
              fitView
              fitViewOptions={{ padding: 0.04, maxZoom: 1 }}
              onInit={setFlow}
              nodesDraggable={false}
              nodesConnectable={false}
              nodesFocusable={false}
              edgesFocusable={false}
              elementsSelectable={false}
              autoPanOnNodeFocus={false}
              panOnDrag
              zoomOnScroll
              zoomOnPinch
              zoomOnDoubleClick={false}
              proOptions={{ hideAttribution: true }}
            >
              <Background color="#39434f" gap={24} size={1} />
              <GraphControls container={controlsRoot} />
            </ReactFlow>
          )}
        </div>
        <footer className="flow-legend">
          <span>
            <ArrowRight size={14} />
            {view === "expected"
              ? "Expected causation"
              : view === "predicted"
                ? "Predicted causation"
                : "Causal link"}
          </span>
          <span>
            <i />
            Fixture / stream context
          </span>
          {uncertain && (
            <span className="uncertain-note">
              Some messages have no resolved cause
            </span>
          )}
          {capture && (
            <span className="graph-caption">
              {capture.fidelity === "exact"
                ? "exact causality"
                : "grouped capture"}
              {capture.settled ? "" : " / partial"}
            </span>
          )}
          {!compact && (
            <div className="graph-controls-slot" ref={setControlsRoot} />
          )}
        </footer>
      </section>
      <MessageInspector
        hidden={!inspectorVisible}
        closable={canvas || compact}
        node={selected}
        parent={nodes.find((node) => node.id === selected?.parentId)}
        view={view}
        onBack={dismissInspector}
      />
    </div>
  )
}

function MessageNode({ data }: NodeProps<MessageFlowNode>) {
  return (
    <div
      data-graph-node
      data-node-id={data.message.id}
      data-context={data.message.context}
      data-status={data.message.status}
      data-subject={data.message.subject}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="message-handle"
      />
      <MessageCard
        node={data.message}
        view={data.view}
        selected={data.selected}
        onSelect={data.onSelect}
      />
      <Handle
        type="source"
        position={Position.Right}
        className="message-handle"
      />
    </div>
  )
}

function MessageCard({
  node,
  view,
  selected,
  onSelect,
}: {
  node: MessageGraphNode
  view: FlowView
  selected: boolean
  onSelect: (id: string) => void
}) {
  const Icon = kindIcons[node.kind]
  return (
    <button
      type="button"
      className={cn(
        "message-card nodrag nopan",
        `message-card-${node.kind}`,
        selected && "message-card-selected",
        node.context && "message-card-fixture"
      )}
      onClick={() => onSelect(node.id)}
      aria-label={`${kindLabels[node.kind]} ${node.name}`}
      aria-pressed={selected}
      data-message-id={node.id}
    >
      <span className="message-card-kind">
        <Icon size={13} />
        {kindLabels[node.kind]}
        {node.context && <span className="fixture-tag">Fixture</span>}
      </span>
      <strong className="message-card-name" title={node.name}>
        {node.name}
      </strong>
      <span className="message-card-facts">
        {messageFacts(node).map(([key, value]) => (
          <span key={key} title={`${key}: ${value}`}>
            <span>{key}</span> <b>{value}</b>
          </span>
        ))}
      </span>
      <span
        className="message-card-footer"
        data-status={view === "expected" || node.context ? "idle" : node.status}
      >
        {messageState(node, view)}
        <span aria-hidden="true">↗</span>
      </span>
    </button>
  )
}

function MessageEdge({
  sourceX,
  sourceY,
  sourcePosition,
  targetX,
  targetY,
  targetPosition,
  source,
  target,
  data,
}: EdgeProps<MessageFlowEdge>) {
  const markerId = `arrow-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`
  const directed = data?.relationship === "causation"
  const [path] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    curvature: 0.3,
  })
  return (
    <g
      data-graph-edge
      data-source-id={source}
      data-target-id={target}
      data-edge-relationship={data?.relationship}
    >
      {directed && (
        <defs>
          <marker
            id={markerId}
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="9"
            markerHeight="9"
            orient="auto"
            markerUnits="userSpaceOnUse"
          >
            <path d="M 1 1 L 9 5 L 1 9" className="graph-edge-arrow" />
          </marker>
        </defs>
      )}
      <path
        d={path}
        className={cn("graph-edge", !directed && "graph-edge-context")}
        vectorEffect="non-scaling-stroke"
        markerEnd={directed ? `url(#${markerId})` : undefined}
      />
    </g>
  )
}

function GraphControls({ container }: { container: HTMLDivElement | null }) {
  const { fitView, zoomIn, zoomOut } = useReactFlow()
  const { zoom } = useViewport()
  return container
    ? createPortal(
        <div className="graph-controls nodrag nopan nowheel">
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void zoomOut({ duration: 150 })}
            aria-label="Zoom out"
          >
            <Minus />
          </Button>
          <span className="graph-zoom-value">{Math.round(zoom * 100)}%</span>
          <Button
            variant="ghost"
            size="icon"
            onClick={() => void zoomIn({ duration: 150 })}
            aria-label="Zoom in"
          >
            <Plus />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={() =>
              void fitView({
                padding: 0.04,
                minZoom: 0.2,
                maxZoom: 1,
                duration: 150,
              })
            }
            aria-label="Fit graph to view"
          >
            <Focus />
          </Button>
        </div>,
        container
      )
    : null
}
