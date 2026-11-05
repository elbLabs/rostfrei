import { readEvidence, createSelection, resolveSelection, renderText } from "@rostfrei/tracer-presentation"
import { jsonPrefix, page, short } from "./detail.js"

// Selection belongs to this single owned instance, never an implicit session.
export function createPresentationAdapter(captures) {
  if (!captures.length || captures.length > 16) throw new Error("Load 1..16 captures")
  const entries = new Map(captures.map(({ captureId, label, evidence }) => {
    if (!/^sha256:[a-f0-9]{64}$/.test(captureId)) throw new Error("Expected capture SHA-256")
    const view = readEvidence(evidence)
    if (view.kind !== "operation") throw new Error("Expected saved operation evidence")
    // Keep identity-bearing wire summaries bounded without shortening domain IDs.
    const ids = [view.snapshotId, view.operation.operationId, ...(view.series?.messageSeries.messages ?? []).map((message) => message.messageId)]
    if (ids.some((id) => typeof id !== "string" || id.length > 256)) throw new Error("Adapter identity limit: 256 characters")
    return [captureId, { captureId, label: short(label, 64), view }]
  }))
  if (entries.size !== captures.length) throw new Error("Duplicate capture digest")
  let selected = null

  function entry(input) {
    const result = entries.get(input.captureId)
    if (!result || input.operationId !== result.view.operation.operationId) throw new Error("Capture/operation is not loaded")
    if (input.snapshotId !== result.view.snapshotId) throw new Error("Stale snapshot; reopen and explicitly reselect")
    return result
  }
  function resolved(input) {
    const item = entry(input)
    // Resolve the original callback identity; never rebind a stale callback.
    const selection = { snapshotId: input.snapshotId, operationId: input.operationId, messageId: input.messageId }
    return { ...item, ...resolveSelection(item.view, selection) }
  }
  function operation(input) {
    const { view } = entry(input)
    return {
      captureId: input.captureId, snapshotId: view.snapshotId, operationId: view.operation.operationId,
      decision: view.operation.result?.decision ?? "not recorded",
      captureStatus: view.captureStatus, provenance: view.provenance,
      diagnosticCount: view.diagnostics.length,
      messageCount: view.series?.messageSeries.messages.length ?? 0,
      capture: view.series ? { settled: view.series.capture.settled, fidelity: view.series.capture.fidelity } : null,
      payloadPolicy: "Hidden; explicit bounded reveal only",
    }
  }
  return {
    listCaptures() {
      return Array.from(entries.values(), ({ captureId, label, view }) => ({
        captureId, snapshotId: view.snapshotId, label, operationId: view.operation.operationId, decision: view.operation.result?.decision ?? "not recorded",
        captureStatus: view.captureStatus, diagnosticCount: view.diagnostics.length,
      }))
    },
    showOperation: operation,
    listMessages(input) {
      const { view } = entry(input)
      const offset = input.offset ?? 0
      if (!Number.isSafeInteger(offset) || offset < 0) throw new Error("Invalid list offset")
      const all = view.series?.messageSeries.messages ?? []
      return { captureStatus: view.captureStatus, messages: all.slice(offset, offset + 16).map(({ messageId, kind, name }) => ({ messageId, selection: createSelection(view, messageId), kind, name: short(name) })), nextOffset: offset + 16 < all.length ? offset + 16 : null }
    },
    captureDetail(input) {
      const { view } = entry(input)
      const detail = jsonPrefix({ captureStatus: view.captureStatus, provenance: view.provenance, diagnostics: view.diagnostics, warnings: view.warnings })
      return { ...page(detail.text, input.offset), truncated: detail.truncated, policy: "Capture diagnostics; payloads hidden" }
    },
    renderOperation(input) {
      const { view, label } = entry(input)
      return `${label}\n${renderText(view, { maxCharacters: 4096 })}`
    },
    inspectMessage(input) {
      const { view, message, outcome, diagnostics } = resolved(input)
      const parent = view.edges.find((edge) => edge.childMessageId === message.messageId)
      const relation = outcome ? `Outcome: ${outcome.outcome.status}`
        : parent ? `Parent matched: ${short(parent.parentMessageId, 26)}`
          : message.causationId ? "No validated parent edge; disconnected" : "No recorded parent/outcome"
      const code = outcome?.outcome.status === "rejected" ? outcome.outcome.value.code : undefined
      return {
        identity: { captureId: input.captureId, snapshotId: view.snapshotId, operationId: input.operationId, messageId: message.messageId },
        captureStatus: view.captureStatus,
        heading: `${short(message.name, 30)} · ${short(message.messageId, 12)}`,
        summary: [
          short(`${message.kind}: ${message.name}`, 46), short(relation, 46),
          ...(typeof code === "string" ? [short(`Code: ${code}`, 46)] : []),
          short(`Capture ${view.captureStatus}: ${view.series?.capture.fidelity ?? "missing"}; settled=${view.series?.capture.settled ?? false}`, 46),
          "Finite capture; downstream completion unproven",
          ...(diagnostics?.length ? ["Evidence diagnostics: open Limitations"] : []),
        ],
        payload: { state: Object.hasOwn(message, "payload") ? "present" : "missing-or-redacted", displayed: false },
      }
    },
    messageDetail(input) {
      const { view, message, outcome, path, diagnostics } = resolved(input)
      let detail
      if (input.section === "identity") {
        detail = jsonPrefix({ captureId: input.captureId, snapshotId: view.snapshotId, operationId: input.operationId, messageId: message.messageId, correlationId: message.correlationId, causationId: message.causationId ?? null, path })
      } else if (input.section === "limitations") {
        detail = jsonPrefix({ captureStatus: view.captureStatus, diagnostics, warnings: view.warnings })
      } else if (input.section === "payload" || input.section === "outcome") {
        if (input.reveal !== true) throw new Error("Explicit reveal required for payload/rejection detail")
        if (input.section === "payload" && !Object.hasOwn(message, "payload")) {
          return { ...page("Payload missing or redacted; no replacement inferred.", input.offset), truncated: false, policy: "missing-or-redacted" }
        }
        if (input.section === "outcome" && !outcome) {
          return { ...page("No validated associated outcome; consult Limitations.", input.offset), truncated: false, policy: "no-associated-outcome" }
        }
        detail = jsonPrefix(input.section === "payload" ? message.payload : outcome)
      } else throw new Error("Unknown detail section")
      return { ...page(detail.text, input.offset), truncated: detail.truncated, policy: input.reveal ? "Explicit reveal of bounded recorded JSON text" : "Payload and rejection details hidden", limits: { characters: detail.limitCharacters, depth: detail.maxDepth, values: detail.maxValues } }
    },
    sourcePage(input) {
      if (input.reveal !== true) throw new Error("Explicit original-source reveal required")
      return { ...page(entry(input).view.sourceText, input.offset, 4096), policy: "Original JSON text; may contain payloads", captureId: input.captureId }
    },
    selectMessage(input) {
      resolved(input) // Validate without cloning or serializing canonical values.
      selected = { captureId: input.captureId, snapshotId: input.snapshotId, operationId: input.operationId, messageId: input.messageId }
      return { ...selected }
    },
    selectedMessage() { return selected ? { ...selected } : null },
  }
}
