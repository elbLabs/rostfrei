import type { FlowView, MessageGraphNode, MessageKind } from "@/lib/types"

export const kindLabels: Record<MessageKind, string> = {
  command: "Command",
  "domain-event": "Domain event",
  "integration-event": "Integration event",
}

export function messageFacts(node: MessageGraphNode): Array<[string, string]> {
  if (node.payload === undefined)
    return [["Payload", "Redacted or unavailable"]]
  if (
    typeof node.payload === "object" &&
    node.payload !== null &&
    !Array.isArray(node.payload)
  ) {
    const entries = Object.entries(node.payload)
    const scalar = entries.filter(
      ([, value]) =>
        value === null || ["string", "number", "boolean"].includes(typeof value)
    )
    if (scalar.length)
      return scalar
        .slice(0, 2)
        .map(([key, value]) => [key, value === "" ? '""' : String(value)])
    const collection = entries.find(([, value]) => Array.isArray(value))
    if (collection)
      return [[collection[0], `${(collection[1] as unknown[]).length} items`]]
    return [
      [
        "Payload",
        entries.length ? `${entries.length} properties` : "Empty object",
      ],
    ]
  }
  if (Array.isArray(node.payload))
    return [["Payload", `${node.payload.length} items`]]
  return [["Value", node.payload === "" ? '""' : String(node.payload)]]
}

export function messageState(node: MessageGraphNode, view: FlowView): string {
  if (node.context) return "Given state"
  if (view === "expected") {
    if (node.expectedOutcome)
      return node.expectedOutcome === "accepted"
        ? "Expect acceptance"
        : "Expect rejection"
    return "Expected message"
  }
  if (node.kind !== "command")
    return view === "running"
      ? "Received"
      : view === "predicted"
        ? "Predicted"
        : "Observed"
  switch (node.status) {
    case "accepted":
      return "Accepted"
    case "rejected":
      return "Rejected"
    case "running":
      return "Awaiting response"
    case "failed":
      return "Result unavailable"
    case "indeterminate":
      return "Outcome unconfirmed"
    default:
      return "Outcome unavailable"
  }
}
