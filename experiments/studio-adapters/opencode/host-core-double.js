// EXPLICIT SYNTHETIC-ONLY CORE DOUBLE for native-host UI qualification while
// the separately owned core package is being rebuilt. Never validates real data.
let snapshots = 0
export function readEvidence(evidence) {
  const source = typeof evidence === "string" ? JSON.parse(evidence) : evidence
  if (source.source !== "synthetic adapter fixture; no Tracer execution") throw new Error("Core double only accepts the synthetic UI fixture")
  const messages = source.messageSeries.messageSeries.messages
  return {
    kind: "operation", snapshotId: `synthetic-ui-only-${++snapshots}`, source,
    sourceText: typeof evidence === "string" ? evidence : JSON.stringify(evidence),
    operation: source.report.operation, series: source.messageSeries, captureStatus: "valid", provenance: "unknown", diagnostics: [],
    edges: messages.filter((m) => messages.some((p) => p.messageId === m.causationId)).map((m) => ({ parentMessageId: m.causationId, childMessageId: m.messageId })),
    warnings: ["Synthetic UI fixture; no runtime evidence.", "Finite capture; downstream completion unproven.", "Grouped unsettled capture; missing parent disconnected."],
  }
}
export function createSelection(view, messageId) {
  return { snapshotId: view.snapshotId, operationId: view.operation.operationId, messageId }
}
export function resolveSelection(view, selection) {
  if (selection.snapshotId !== view.snapshotId || selection.operationId !== view.operation.operationId) throw new Error("Wrong fixture snapshot")
  const message = view.series.messageSeries.messages.find((item) => item.messageId === selection.messageId)
  if (!message) throw new Error("Message missing")
  return { message, outcome: view.series.messageSeries.commandOutcomes.find((item) => item.commandMessageId === message.messageId), diagnostics: [], path: `/messageSeries/messageSeries/messages/${view.series.messageSeries.messages.indexOf(message)}` }
}
export function renderText() { return "Synthetic UI-only core double; no Tracer execution; downstream completion unproven." }
