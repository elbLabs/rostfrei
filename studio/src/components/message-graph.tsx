import { useEffect, useMemo, useRef, useState } from "react"
import {
  Check,
  CircleDot,
  Copy,
  Focus,
  GitBranch,
  ListTree,
  Minus,
  Plus,
  Radio,
  Reply,
} from "lucide-react"
import {
  Handle,
  Panel,
  Position,
  ReactFlow,
  getBezierPath,
  useReactFlow,
  useViewport,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
} from "@xyflow/react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover"
import { layoutMessageGraph, type PositionedNode } from "@/lib/graph"
import type { MessageGraphNode, MessageKind } from "@/lib/types"
import { cn } from "@/lib/utils"

interface MessageGraphProps {
  nodes: MessageGraphNode[]
  running: boolean
}

type MessageNodeData = Record<string, unknown> & {
  message: PositionedNode
}

type MessageFlowNode = Node<MessageNodeData, "message">

type MessageEdgeData = Record<string, unknown> & {
  fidelity: "exact" | "grouped"
  context: boolean
  running: boolean
}

type MessageFlowEdge = Edge<MessageEdgeData, "message">

const kindLabel: Record<MessageKind, string> = {
  command: "command",
  "domain-event": "domain event",
  "integration-event": "integration event",
}

const kindIcon = {
  command: CircleDot,
  "domain-event": GitBranch,
  "integration-event": Radio,
} satisfies Record<MessageKind, typeof CircleDot>

const nodeTypes = { message: MessageNode }
const edgeTypes = { message: MessageEdge }
const enableNodePointerEvents = () => undefined
const DISMISS_MESSAGE_POPUPS_EVENT = "dismiss-message-popups"

export function MessageGraph({ nodes, running }: MessageGraphProps) {
  const layout = useMemo(() => layoutMessageGraph(nodes), [nodes])
  const flowNodes = useMemo<MessageFlowNode[]>(
    () =>
      layout.nodes.map((node) => ({
        id: node.id,
        type: "message",
        position: { x: node.x, y: node.y },
        data: { message: node },
        draggable: false,
        selectable: false,
        focusable: false,
        ariaLabel: `${messageKindLabel(node)} ${node.name}`,
      })),
    [layout.nodes]
  )
  const flowEdges = useMemo<MessageFlowEdge[]>(
    () =>
      layout.edges.map((edge) => ({
        id: edge.id,
        type: "message",
        source: edge.source.id,
        target: edge.target.id,
        selectable: false,
        focusable: false,
        data: { fidelity: edge.fidelity, context: edge.context, running },
      })),
    [layout.edges, running]
  )

  return (
    <section
      className="message-graph"
      aria-label="Directed message series graph"
      onPointerDownCapture={(event) => {
        if (
          event.button === 0 &&
          event.target instanceof Element &&
          event.target.classList.contains("react-flow__pane")
        ) {
          window.dispatchEvent(new Event(DISMISS_MESSAGE_POPUPS_EVENT))
        }
      }}
    >
      <ReactFlow<MessageFlowNode, MessageFlowEdge>
        className="message-flow"
        nodes={flowNodes}
        edges={flowEdges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        nodeOrigin={[0.5, 0.5]}
        minZoom={0.25}
        maxZoom={1.8}
        fitView
        fitViewOptions={{
          padding: 0.28,
          minZoom: 0.5,
          maxZoom: 1,
          duration: 450,
        }}
        nodesDraggable={false}
        nodesConnectable={false}
        nodesFocusable={false}
        edgesFocusable={false}
        elementsSelectable={false}
        onNodeMouseEnter={enableNodePointerEvents}
        autoPanOnNodeFocus={false}
        panOnDrag
        panOnScroll={false}
        zoomOnScroll
        zoomOnPinch
        zoomOnDoubleClick={false}
        onlyRenderVisibleElements
        proOptions={{ hideAttribution: true }}
      >
        <GraphControls />
      </ReactFlow>

      {layout.nodes.length === 0 && (
        <div className="graph-empty">No messages observed yet.</div>
      )}
    </section>
  )
}

function MessageNode({ data }: NodeProps<MessageFlowNode>) {
  const node = data.message
  const Icon = kindIcon[node.kind]
  const label = messageKindLabel(node)
  const [popupOpen, setPopupOpen] = useState(false)
  const [popupPinned, setPopupPinned] = useState(false)
  const closeTimer = useRef<number | undefined>(undefined)

  const cancelClose = () => {
    if (closeTimer.current !== undefined) {
      window.clearTimeout(closeTimer.current)
      closeTimer.current = undefined
    }
  }
  const previewPopup = () => {
    cancelClose()
    setPopupOpen(true)
  }
  const closePreview = () => {
    if (popupPinned) return
    cancelClose()
    closeTimer.current = window.setTimeout(() => setPopupOpen(false), 120)
  }
  const togglePinned = () => {
    cancelClose()
    if (popupPinned) {
      setPopupPinned(false)
      setPopupOpen(false)
    } else {
      setPopupPinned(true)
      setPopupOpen(true)
    }
  }
  const dismissPopup = () => {
    cancelClose()
    setPopupPinned(false)
    setPopupOpen(false)
  }

  useEffect(() => {
    const dismiss = () => {
      if (closeTimer.current !== undefined) {
        window.clearTimeout(closeTimer.current)
      }
      setPopupPinned(false)
      setPopupOpen(false)
    }
    window.addEventListener(DISMISS_MESSAGE_POPUPS_EVENT, dismiss)
    return () => {
      window.removeEventListener(DISMISS_MESSAGE_POPUPS_EVENT, dismiss)
      if (closeTimer.current !== undefined) {
        window.clearTimeout(closeTimer.current)
      }
    }
  }, [])

  return (
    <div
      className={cn(
        "message-flow-node",
        node.context && "message-flow-node-context"
      )}
      data-graph-node
      data-node-id={node.id}
      data-parent-id={node.parentId}
      data-layout-x={node.x}
      data-layout-y={node.y}
      data-context={node.context}
      style={{ animationDelay: node.parentId ? "600ms" : "0ms" }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="message-handle"
      />
      <Handle
        type="source"
        position={Position.Right}
        className="message-handle"
      />
      <Popover open={popupOpen} onOpenChange={() => undefined}>
        <PopoverAnchor asChild>
          <button
            type="button"
            className={cn(
              "message-node nodrag nopan",
              `message-node-${node.kind}`,
              node.status === "running" && "message-node-running"
            )}
            aria-label={`${label} ${node.name}`}
            aria-haspopup="dialog"
            aria-expanded={popupOpen}
            onPointerEnter={previewPopup}
            onPointerLeave={closePreview}
            onFocus={previewPopup}
            onBlur={closePreview}
            onPointerDown={(event) => {
              event.stopPropagation()
              if (event.button === 0) togglePinned()
            }}
            onClick={(event) => {
              event.stopPropagation()
              if (event.detail === 0) togglePinned()
            }}
          >
            <span className="message-node-glint" />
          </button>
        </PopoverAnchor>
        <PopoverContent
          data-node-popup
          data-popup-pinned={popupPinned}
          className={cn(
            node.kind === "command" && "w-[min(380px,calc(100vw-2rem))]"
          )}
          side="top"
          align="start"
          onPointerEnter={cancelClose}
          onPointerLeave={closePreview}
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => event.preventDefault()}
          onPointerDownOutside={(event) => {
            event.preventDefault()
          }}
          onEscapeKeyDown={dismissPopup}
        >
          <div className="flex items-start gap-5 border-b border-white/8 px-3.5 py-3">
            <div className="min-w-0">
              <div className="mb-1 flex items-center gap-1.5 text-[10px] tracking-[0.12em] text-white/42 uppercase">
                <Icon className="size-3" />
                {label}
              </div>
              <div className="truncate font-mono text-[13px] text-white/92">
                {node.name}
              </div>
            </div>
          </div>

          <div className="px-3.5 py-3">
            <div className="mb-2 flex items-center gap-1.5 font-mono text-[9px] tracking-[0.13em] text-white/34 uppercase">
              <ListTree className="size-3" />
              {node.kind === "command" ? "request" : "payload"}
            </div>
            {node.payload === undefined ? (
              <p className="m-0 font-mono text-[11px] text-white/34 italic">
                redacted or empty
              </p>
            ) : (
              <PayloadList payload={node.payload} />
            )}
          </div>

          {node.kind === "command" && !node.context && (
            <div
              data-command-response
              className="border-t border-white/7 px-3.5 py-3"
            >
              <div className="mb-2 flex items-center gap-1.5 font-mono text-[9px] tracking-[0.13em] text-white/34 uppercase">
                <Reply className="size-3" />
                response
                <Badge
                  variant={
                    node.status === "accepted"
                      ? "success"
                      : node.status === "rejected" || node.status === "failed"
                        ? "danger"
                        : "neutral"
                  }
                  className="ml-auto"
                >
                  {node.status === "running"
                    ? "pending"
                    : (node.status ?? "unavailable")}
                </Badge>
              </div>
              {node.response === undefined ? (
                <p className="m-0 font-mono text-[11px] text-white/34 italic">
                  {node.status === "running"
                    ? "awaiting command response"
                    : "response redacted or unavailable"}
                </p>
              ) : (
                <PayloadList payload={node.response} omitTechnicalIdentities />
              )}
            </div>
          )}

          {node.kind !== "command" && (node.messageId || node.causationId) && (
            <div className="flex gap-1.5 border-t border-white/7 px-3.5 py-2.5">
              {node.messageId && (
                <CopyIdentityButton label="message ID" value={node.messageId} />
              )}
              {node.causationId && (
                <CopyIdentityButton label="cause ID" value={node.causationId} />
              )}
            </div>
          )}
        </PopoverContent>
      </Popover>

      <div className="message-node-label" aria-hidden="true">
        <span>{node.name}</span>
        <small>{label}</small>
      </div>
    </div>
  )
}

function messageKindLabel(node: MessageGraphNode): string {
  return node.context === "fixture"
    ? "domain event series"
    : kindLabel[node.kind]
}

function CopyIdentityButton({
  label,
  value,
}: {
  label: string
  value: string
}) {
  const [copied, setCopied] = useState(false)
  const resetTimer = useRef<number | undefined>(undefined)

  useEffect(
    () => () => {
      if (resetTimer.current !== undefined) {
        window.clearTimeout(resetTimer.current)
      }
    },
    []
  )

  const copyIdentity = async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      if (resetTimer.current !== undefined) {
        window.clearTimeout(resetTimer.current)
      }
      resetTimer.current = window.setTimeout(() => setCopied(false), 1400)
    } catch {
      setCopied(false)
    }
  }

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="h-6 px-2 font-mono text-[9px] font-normal text-white/45"
      data-copy-identity={label}
      aria-label={`Copy ${label}`}
      onClick={() => void copyIdentity()}
    >
      {copied ? <Check /> : <Copy />}
      {copied ? "copied" : label}
    </Button>
  )
}

function MessageEdge({
  id,
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
  const markerId = `message-arrow-${id.replace(/[^a-zA-Z0-9_-]/g, "-")}`
  const [path] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    curvature: 0.42,
  })

  return (
    <g data-graph-edge data-source-id={source} data-target-id={target}>
      <defs>
        <marker
          id={markerId}
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerWidth="10"
          markerHeight="10"
          markerUnits="userSpaceOnUse"
          orient="auto"
        >
          <path
            d="M 1 1.25 L 9 5 L 1 8.75 L 3.4 5 Z"
            className={cn(
              "graph-edge-arrow",
              data?.context && "graph-edge-arrow-context"
            )}
          />
        </marker>
      </defs>
      <path
        d={path}
        className={cn(
          "graph-edge",
          data?.fidelity === "grouped" && "graph-edge-grouped",
          data?.context && "graph-edge-context",
          data?.running && "graph-edge-running"
        )}
        pathLength="1"
        vectorEffect="non-scaling-stroke"
        markerEnd={`url(#${markerId})`}
      />
    </g>
  )
}

function GraphControls() {
  const { fitView, zoomIn, zoomOut } = useReactFlow<
    MessageFlowNode,
    MessageFlowEdge
  >()
  const { zoom } = useViewport()

  return (
    <Panel
      position="bottom-right"
      className="graph-controls nodrag nopan nowheel"
    >
      <Button
        variant="ghost"
        size="icon-sm"
        onClick={() => void zoomOut({ duration: 180 })}
        aria-label="Zoom out"
      >
        <Minus />
      </Button>
      <span className="graph-zoom-value">{Math.round(zoom * 100)}%</span>
      <Button
        variant="ghost"
        size="icon-sm"
        onClick={() => void zoomIn({ duration: 180 })}
        aria-label="Zoom in"
      >
        <Plus />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        onClick={() =>
          void fitView({
            padding: 0.28,
            minZoom: 0.5,
            maxZoom: 1,
            duration: 350,
          })
        }
        aria-label="Fit graph to view"
      >
        <Focus />
      </Button>
    </Panel>
  )
}

interface PayloadRow {
  path: string
  value: string
  kind: "string" | "number" | "boolean" | "null" | "empty"
}

function PayloadList({
  payload,
  omitTechnicalIdentities = false,
}: {
  payload: unknown
  omitTechnicalIdentities?: boolean
}) {
  const rows = flattenPayload(payload).filter(
    (row) => !omitTechnicalIdentities || !isTechnicalIdentity(row.path)
  )
  return (
    <dl className="payload-list">
      {rows.map((row, index) => (
        <div className="payload-row" key={`${row.path}-${index}`}>
          <dt>{row.path}</dt>
          <dd data-kind={row.kind}>{row.value}</dd>
        </div>
      ))}
    </dl>
  )
}

function flattenPayload(
  value: unknown,
  path = "value",
  rows: PayloadRow[] = []
): PayloadRow[] {
  if (value === null) {
    rows.push({ path, value: "null", kind: "null" })
    return rows
  }
  if (Array.isArray(value)) {
    if (value.length === 0)
      rows.push({ path, value: "empty list", kind: "empty" })
    value.forEach((item, index) =>
      flattenPayload(item, `${path}[${index}]`, rows)
    )
    return rows
  }
  if (typeof value === "object") {
    const entries = Object.entries(value)
    if (entries.length === 0)
      rows.push({ path, value: "empty object", kind: "empty" })
    entries.forEach(([key, item]) =>
      flattenPayload(item, path === "value" ? key : `${path}.${key}`, rows)
    )
    return rows
  }

  const kind = typeof value
  if (kind === "string" || kind === "number" || kind === "boolean") {
    rows.push({ path, value: String(value), kind })
  } else {
    rows.push({ path, value: String(value), kind: "empty" })
  }
  return rows
}

function isTechnicalIdentity(path: string): boolean {
  const property = path.split(".").at(-1)
  return (
    property === "messageId" ||
    property === "causationId" ||
    property === "commandMessageId" ||
    property === "responseMessageId"
  )
}
