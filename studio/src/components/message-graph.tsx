import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { createPortal } from "react-dom"
import { useTheme } from "@rostfrei/ui/theme"
import {
  ArrowRight,
  CircleDot,
  GitBranch,
  PanelRight,
  Radio,
  Workflow,
} from "lucide-react"
import {
  Handle,
  Position,
  ReactFlow,
  getBezierPath,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
  type ReactFlowInstance,
} from "@xyflow/react"

import { GraphCard } from "@/components/graph-card"
import { MessageInspector } from "@/components/message-inspector"
import { GraphBackground, GraphControls, GraphHeading } from "./graph-tools"
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
  OperationMessageSeries,
} from "@/lib/types"
import { useMediaQuery } from "@/lib/use-media-query"
import { cn } from "@/lib/utils"

interface MessageGraphProps {
  toolbar?: ReactNode
  summary?: ReactNode
  nodes: MessageGraphNode[]
  view: FlowView
  emptyMessage?: string
  emptyTitle?: string
  capture?: OperationMessageSeries["capture"]
}

type MessageFlowNode = Node<
  {
    message: MessageGraphNode
    view: FlowView
    selected: boolean
    contextTone?: number
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
  toolbar,
  summary,
  nodes,
  view,
  emptyMessage,
  emptyTitle = "No flow to display",
  capture,
}: MessageGraphProps) {
  const { resolvedTheme } = useTheme()
  const compact = useMediaQuery("(max-width: 1100px)")
  const [selectedId, setSelectedId] = useState<string>()
  const [detailsOpen, setDetailsOpen] = useState(false)
  const inspectorVisible = detailsOpen
  const [controlsRoot, setControlsRoot] = useState<HTMLDivElement | null>(null)
  const chromeRef = useRef<HTMLDivElement>(null)
  const viewportRef = useRef<HTMLDivElement>(null)
  const mobileTrigger = useRef<HTMLElement | null>(null)
  const [flow, setFlow] = useState<ReactFlowInstance<
    MessageFlowNode,
    MessageFlowEdge
  > | null>(null)
  const layout = useMemo(() => layoutMessageGraph(nodes), [nodes])
  const contextTones = useMemo(() => {
    const tones = new Map<string, number>()
    if (view === "retained")
      for (const node of nodes)
        if (node.boundedContext && !tones.has(node.boundedContext))
          tones.set(node.boundedContext, tones.size % 4)
    return tones
  }, [nodes, view])
  const selected =
    nodes.find((node) => node.id === selectedId) ??
    nodes.find((node) => node.subject === true) ??
    nodes.find(
      (node) =>
        node.kind === "command" && !node.context && node.subject !== false
    ) ??
    nodes[0]
  const highlightedId = inspectorVisible ? selected?.id : undefined
  const select = useCallback((id: string) => {
    mobileTrigger.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null
    setSelectedId(id)
    setDetailsOpen(true)
  }, [])
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
          contextTone: node.boundedContext
            ? contextTones.get(node.boundedContext)
            : undefined,
          onSelect: select,
        },
        draggable: false,
        selectable: false,
        focusable: false,
      })),
    [layout.nodes, view, highlightedId, select, contextTones]
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
  const fit = useCallback(
    (duration = 0) => {
      if (!flow || !viewportRef.current || !chromeRef.current) return
      const viewport = viewportRef.current.getBoundingClientRect()
      const chrome = chromeRef.current.getBoundingClientRect()
      // Keep the fitted graph clear of the floating summary without cropping the pane.
      void flow.fitView({
        padding: {
          top: `${chrome.bottom - viewport.top + 12}px`,
          x: "4%",
          bottom: "4%",
        },
        minZoom: 0.2,
        maxZoom: 1,
        duration,
      })
    },
    [flow]
  )
  const autoFit = useCallback(() => {
    if (view !== "running") fit()
  }, [fit, view])

  useEffect(() => {
    const frame = window.requestAnimationFrame(autoFit)
    return () => window.cancelAnimationFrame(frame)
  }, [autoFit, nodeIds, compact])

  useEffect(() => {
    const element = viewportRef.current
    if (!element || compact) return
    let frame = 0
    const observer = new ResizeObserver(() => {
      window.cancelAnimationFrame(frame)
      frame = window.requestAnimationFrame(autoFit)
    })
    observer.observe(element)
    return () => {
      observer.disconnect()
      window.cancelAnimationFrame(frame)
    }
  }, [autoFit, compact])

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
            ? "Historical simulation (uncommitted)"
            : view === "retained"
              ? "Retained correlation events"
              : "Observed flow"
  const dismissInspector = () => {
    setDetailsOpen(false)
    window.requestAnimationFrame(() => mobileTrigger.current?.focus())
  }
  const overlayInspector = !compact
  const inspector = (
    <MessageInspector
      hidden={!inspectorVisible}
      closable
      node={selected}
      parent={nodes.find((node) => node.id === selected?.parentId)}
      siblings={
        selected?.causationId && !selected.parentId
          ? nodes.filter(
              (node) =>
                node.id !== selected.id &&
                node.causationId === selected.causationId
            ).length
          : 0
      }
      view={view}
      onBack={dismissInspector}
    />
  )

  return (
    <div
      className={cn(
        "graph-workspace",
        compact && detailsOpen && "mobile-inspecting"
      )}
    >
      <div className="graph-chrome" ref={chromeRef}>
        {toolbar}
        <GraphHeading title={title} icon={<Workflow size={16} />}>
          <span>
            {nodes.length - fixtureCount}{" "}
            {nodes.length - fixtureCount === 1 ? "message" : "messages"}
            {fixtureCount > 0 && (
              <>
                , {fixtureCount} fixture{" "}
                {fixtureCount === 1 ? "event" : "events"}
              </>
            )}
          </span>
          {!compact && nodes.length > 0 && (
            <div className="graph-controls-slot" ref={setControlsRoot} />
          )}
          {!compact && (
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
                  setDetailsOpen(true)
                }
              }}
            >
              <PanelRight /> Details
            </Button>
          )}
        </GraphHeading>
        {summary}
      </div>
      <section className="flow-panel" aria-label={title}>
        <div className="message-graph" ref={viewportRef}>
          {nodes.length === 0 ? (
            <div className="graph-empty">
              <Workflow />
              <h3>{emptyTitle}</h3>
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
                        ? "Fixture state"
                        : node.edgeRelationship === "context"
                          ? "Root command"
                          : node.subject === true
                            ? "Root command"
                            : parent && node.edgeFidelity === "exact"
                              ? `${view === "expected" ? "Expected after" : "Caused by"} ${parent.name}`
                              : view === "retained"
                                ? `${node.boundedContext ?? "Unknown context"}: ${node.causationId && node.causationId === node.messageId ? "origin" : "cause not returned"}`
                                : "No resolved cause"}
                    </p>
                    <MessageCard
                      node={node}
                      view={view}
                      selected={highlightedId === node.id}
                      contextTone={
                        node.boundedContext
                          ? contextTones.get(node.boundedContext)
                          : undefined
                      }
                      onSelect={select}
                    />
                  </li>
                )
              })}
            </ol>
          ) : (
            <ReactFlow<MessageFlowNode, MessageFlowEdge>
              className="message-flow"
              colorMode={resolvedTheme}
              nodes={flowNodes}
              edges={flowEdges}
              nodeTypes={nodeTypes}
              edgeTypes={edgeTypes}
              minZoom={0.2}
              maxZoom={1.5}
              fitView
              fitViewOptions={{ padding: 0.04, maxZoom: 1 }}
              onInit={setFlow}
              onPaneClick={() => setDetailsOpen(false)}
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
              <GraphBackground />
              {controlsRoot &&
                createPortal(
                  <GraphControls onFit={() => fit(150)} />,
                  controlsRoot
                )}
            </ReactFlow>
          )}
          {overlayInspector && inspector}
        </div>
        <footer className="flow-legend">
          <span>
            <ArrowRight size={14} />
            {view === "expected"
              ? "Expected causation"
              : view === "predicted"
                ? "Predicted causation"
                : view === "retained"
                  ? "Recorded causation"
                  : "Causal link"}
          </span>
          <span>
            <i />
            Fixture / stream context
          </span>
          {uncertain && (
            <span className="uncertain-note">
              {view === "retained"
                ? "Some causes are not in these retained events"
                : "Some messages have no resolved cause"}
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
        </footer>
      </section>
      {!overlayInspector && inspector}
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
      data-bounded-context={data.message.boundedContext}
      data-message-kind={data.message.kind}
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
        contextTone={data.contextTone}
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
  contextTone,
  onSelect,
}: {
  node: MessageGraphNode
  view: FlowView
  selected: boolean
  contextTone?: number
  onSelect: (id: string) => void
}) {
  const Icon = kindIcons[node.kind]
  return (
    <GraphCard
      kind={kindLabels[node.kind]}
      icon={<Icon size={13} />}
      kindTag={
        node.context ? (
          <span className="fixture-tag">Fixture</span>
        ) : view === "retained" && node.boundedContext ? (
          <span
            className="context-tag"
            data-context-tone={contextTone}
            title={`Bounded context ${node.boundedContext}`}
          >
            {node.boundedContext}
          </span>
        ) : null
      }
      label={node.name}
      facts={messageFacts(node, view)}
      footer={messageState(node, view)}
      footerStatus={view === "expected" || node.context ? "idle" : node.status}
      className={cn(
        `message-card-${node.kind}`,
        node.context && "message-card-fixture"
      )}
      onClick={() => onSelect(node.id)}
      accessibleLabel={`${kindLabels[node.kind]} ${node.name}`}
      selected={selected}
      attributes={{ "data-message-id": node.id }}
    />
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
