import {
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react"
import { useTheme } from "@rostfrei/ui/theme"

type DomainNodeData = {
  kind: "command" | "policy" | "decision" | "event" | "rejection"
  title: string
  description: string
  input?: boolean
  output?: "single" | "branch"
}

type DomainNode = Node<DomainNodeData, "domain">

const handleStyle = {
  width: 9,
  height: 9,
  border: "2px solid var(--card)",
  background: "var(--primary)",
}

function DomainNodeCard({ data }: NodeProps<DomainNode>) {
  return (
    <div className="w-48 border border-border bg-card px-4 py-3 text-left">
      {data.input ? (
        <Handle
          type="target"
          position={Position.Left}
          style={handleStyle}
          isConnectable={false}
        />
      ) : null}

      <p
        className="font-mono text-[9px] tracking-[0.14em] uppercase"
        style={{
          color:
            data.kind === "command"
              ? "var(--command)"
              : data.kind === "event"
                ? "var(--domain-event)"
                : data.kind === "rejection"
                  ? "var(--destructive)"
                  : "var(--muted-foreground)",
        }}
      >
        {data.kind === "event" ? "domain event" : data.kind}
      </p>
      <p className="mt-1 text-sm font-semibold text-card-foreground">
        {data.title}
      </p>
      <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
        {data.description}
      </p>

      {data.output === "single" ? (
        <Handle
          type="source"
          position={Position.Right}
          style={handleStyle}
          isConnectable={false}
        />
      ) : null}

      {data.output === "branch" ? (
        <>
          <Handle
            id="accepted"
            type="source"
            position={Position.Right}
            style={{ ...handleStyle, top: "35%" }}
            isConnectable={false}
          />
          <Handle
            id="rejected"
            type="source"
            position={Position.Right}
            style={{
              ...handleStyle,
              top: "70%",
              background: "var(--destructive)",
            }}
            isConnectable={false}
          />
        </>
      ) : null}
    </div>
  )
}

const nodeTypes = {
  domain: DomainNodeCard,
}

const initialNodes: DomainNode[] = [
  {
    id: "command",
    type: "domain",
    position: { x: 0, y: 105 },
    data: {
      kind: "command",
      title: "Rent bicycle",
      description: "Carries the fleet and bicycle IDs.",
      output: "single",
    },
  },
  {
    id: "policy",
    type: "domain",
    position: { x: 245, y: 105 },
    data: {
      kind: "policy",
      title: "Assess eligibility",
      description: "Checks rental status and bicycle condition.",
      input: true,
      output: "single",
    },
  },
  {
    id: "decision",
    type: "domain",
    position: { x: 490, y: 105 },
    data: {
      kind: "decision",
      title: "Decide rental",
      description: "Raises BicycleRented or returns a rejection.",
      input: true,
      output: "branch",
    },
  },
  {
    id: "accepted",
    type: "domain",
    position: { x: 770, y: 15 },
    data: {
      kind: "event",
      title: "Bicycle rented",
      description: "Appended to fleet history on acceptance.",
      input: true,
    },
  },
  {
    id: "rejected",
    type: "domain",
    position: { x: 770, y: 205 },
    data: {
      kind: "rejection",
      title: "Rental rejected",
      description: "Declines the rental without appending events.",
      input: true,
    },
  },
]

const initialEdges: Edge[] = [
  {
    id: "command-policy",
    source: "command",
    target: "policy",
    type: "smoothstep",
    markerEnd: { type: MarkerType.ArrowClosed },
  },
  {
    id: "policy-decision",
    source: "policy",
    target: "decision",
    type: "smoothstep",
    markerEnd: { type: MarkerType.ArrowClosed },
  },
  {
    id: "decision-accepted",
    source: "decision",
    sourceHandle: "accepted",
    target: "accepted",
    type: "smoothstep",
    label: "accepted",
    animated: true,
    markerEnd: { type: MarkerType.ArrowClosed },
  },
  {
    id: "decision-rejected",
    source: "decision",
    sourceHandle: "rejected",
    target: "rejected",
    type: "smoothstep",
    label: "rejected",
    markerEnd: { type: MarkerType.ArrowClosed },
  },
]

export function DomainFlow() {
  const { resolvedTheme } = useTheme()
  return (
    <div className="my-6 overflow-hidden border border-border bg-muted">
      <div className="border-b border-border/70 px-4 py-3">
        <p className="font-mono text-[10px] tracking-[0.14em] text-muted-foreground uppercase">
          Rental decision
        </p>
      </div>
      <div className="h-96 w-full sm:h-105">
        <ReactFlow<DomainNode>
          aria-label="A rental action evaluates eligibility, then raises a domain event or returns a rejection"
          colorMode={resolvedTheme}
          defaultEdgeOptions={{
            style: { stroke: "var(--muted-foreground)" },
            labelStyle: { fill: "var(--foreground)" },
            labelBgStyle: { fill: "var(--card)" },
          }}
          defaultEdges={initialEdges}
          defaultNodes={initialNodes}
          edgesFocusable={false}
          fitView
          fitViewOptions={{ padding: 0.2 }}
          maxZoom={1.4}
          minZoom={0.45}
          nodeTypes={nodeTypes}
          nodesConnectable={false}
          panOnScroll={false}
          preventScrolling={false}
          zoomOnDoubleClick={false}
          zoomOnScroll={false}
        >
          <Background
            color="var(--border)"
            gap={18}
            size={1}
            variant={BackgroundVariant.Dots}
          />
          <Controls position="bottom-right" showInteractive={false} />
        </ReactFlow>
      </div>
      <p className="border-t border-border/70 px-4 py-2.5 text-xs text-muted-foreground">
        Drag the nodes or use the controls to inspect the flow.
      </p>
    </div>
  )
}
