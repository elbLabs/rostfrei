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

type DomainNodeData = {
  kind: "command" | "decision" | "event" | "rejection"
  title: string
  description: string
  input?: boolean
  output?: "single" | "branch"
}

type DomainNode = Node<DomainNodeData, "domain">

const handleStyle = {
  width: 9,
  height: 9,
  border: "2px solid #100e0a",
  background: "#ff6b4a",
}

function DomainNodeCard({ data }: NodeProps<DomainNode>) {
  return (
    <div className="w-48 rounded-lg border border-border bg-card px-4 py-3 text-left shadow-lg shadow-black/20">
      {data.input ? (
        <Handle
          type="target"
          position={Position.Left}
          style={handleStyle}
          isConnectable={false}
        />
      ) : null}

      <p className="font-mono text-[9px] tracking-[0.14em] text-primary uppercase">
        {data.kind}
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
            style={{ ...handleStyle, top: "70%", background: "#a99f8d" }}
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
      description: "Carries the rider and bicycle identities.",
      output: "single",
    },
  },
  {
    id: "decision",
    type: "domain",
    position: { x: 280, y: 105 },
    data: {
      kind: "decision",
      title: "Assess eligibility",
      description: "Evaluates the rule without changing state.",
      input: true,
      output: "branch",
    },
  },
  {
    id: "accepted",
    type: "domain",
    position: { x: 575, y: 15 },
    data: {
      kind: "event",
      title: "Bicycle rented",
      description: "Records the accepted state transition.",
      input: true,
    },
  },
  {
    id: "rejected",
    type: "domain",
    position: { x: 575, y: 205 },
    data: {
      kind: "rejection",
      title: "Rental rejected",
      description: "Returns an explicit domain outcome.",
      input: true,
    },
  },
]

const initialEdges: Edge[] = [
  {
    id: "command-decision",
    source: "command",
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
  return (
    <div className="my-8 overflow-hidden rounded-xl border border-border bg-[#0d0b08] shadow-lg shadow-black/10">
      <div className="border-b border-border/70 px-4 py-3">
        <p className="font-mono text-[10px] tracking-[0.14em] text-muted-foreground uppercase">
          Interactive domain flow
        </p>
      </div>
      <div className="h-96 w-full sm:h-105">
        <ReactFlow<DomainNode>
          aria-label="A command flows into a decision that produces either an accepted event or a rejection"
          colorMode="dark"
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
            color="#332a1f"
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
