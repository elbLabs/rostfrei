import { Plugin } from "@opencode/plugin/tui"
import { Presentation } from "./rpc.js"
import { short } from "./detail.js"

// Native, bounded, keyboard-navigable pages. State is one owned instance only.
export function installInspector(context) {
  let busy = false
  let disposed = false
  const messageOffsets = new Map()
  const active = () => { if (disposed) throw new Error("Inspector unloaded") }
  async function run() {
    if (busy || disposed) return
    busy = true
    try {
      const client = context.client.rpc(Presentation)
      const options = { location: context.location ?? context.data.location.default() }
      async function call(method, input) {
        const value = await client[method](input, options)
        active()
        return value
      }
      async function choose(input) {
        context.ui.dialog.set?.({ size: "large", centered: true })
        const value = await context.ui.dialog.select(input)
        active()
        return value
      }
      async function detail(identity, section) {
        let offset = 0
        while (true) {
          const data = section === "capture" ? await call("captureDetail", { ...identity, offset })
            : await call("messageDetail", { ...identity, section, offset, reveal: section === "payload" || section === "outcome" })
          const lines = data.text.match(/[\s\S]{1,40}/g) ?? ["(empty)"]
          const choice = await choose({
            title: `${section === "payload" ? "Payload revealed" : section} · page ${Math.floor(offset / 160) + 1}`,
            options: [
              ...lines.map((title, index) => ({ title, value: `line-${index}` })),
              { title: data.truncated ? "Bounded JSON prefix; remainder omitted" : "Recorded text; no inferred values", value: "policy" },
              ...(data.nextOffset !== null ? [{ title: "Next page", value: "next" }] : []),
              ...(offset ? [{ title: "Previous page", value: "previous" }] : []),
              { title: section === "capture" ? "Back to messages" : "Back to selected summary", value: "back" },
            ],
          })
          if (!choice || choice === "back") return
          if (choice.startsWith("line-") || choice === "policy") continue
          offset = choice === "next" ? data.nextOffset : Math.max(0, offset - 160)
        }
      }
      async function inspect(identity) {
        // Prepare readable content before committing a changed selection.
        const selected = await call("inspectMessage", identity)
        await call("selectMessage", identity)
        while (true) {
          const action = await choose({
            title: `Selected: ${selected.heading}`,
            options: [
              ...selected.summary.map((title, index) => ({ title, value: `summary-${index}` })),
              { title: selected.payload.state === "present" ? "Payload hidden until explicit reveal" : "Payload missing or redacted", value: "policy" },
              { title: "Exact IDs", value: "identity" },
              { title: "Limitations", value: "limitations" },
              { title: "Reveal payload", value: "payload", disabled: selected.payload.state !== "present" },
              { title: "Reveal recorded outcome", value: "outcome" },
              { title: "Back to messages", value: "back" },
            ],
          })
          if (!action || action === "back") return
          if (action.startsWith("summary-") || action === "policy") continue
          await detail(identity, action)
        }
      }
      const captures = await call("listCaptures", {})
      let selection = await call("selectedMessage", {})
      while (true) {
        const captureId = await choose({
          title: "Saved Tracer captures (owned instance)",
          current: selection?.captureId,
          options: captures.map((capture) => ({ title: `${capture.label} · ${capture.decision}`, value: capture.captureId, description: short(capture.captureId, 24) })),
        })
        if (!captureId) return
        const capture = captures.find((item) => item.captureId === captureId)
        const operation = { captureId, snapshotId: capture.snapshotId, operationId: capture.operationId }
        let offset = messageOffsets.get(capture.snapshotId) ?? 0
        while (true) {
          const page = await call("listMessages", { ...operation, offset })
          selection = await call("selectedMessage", {})
          const messageId = await choose({
            title: `Messages · ${capture.decision} · ${page.captureStatus}`,
            current: selection?.captureId === captureId ? selection.messageId : undefined,
            options: [
              ...page.messages.map((message) => ({ title: `${short(message.name, 28)}${selection?.captureId === captureId && selection.messageId === message.messageId ? " (selected)" : ""}`, value: message.messageId, description: `${message.kind} · ${short(message.messageId, 16)}` })),
              ...(page.nextOffset !== null ? [{ title: "Next messages", value: "@next" }] : []),
              ...(offset ? [{ title: "Previous messages", value: "@previous" }] : []),
              { title: `Capture ${page.captureStatus}: diagnostics`, value: "@diagnostics" },
            ],
          })
          if (!messageId) break
          if (messageId === "@diagnostics") { await detail(operation, "capture"); continue }
          if (messageId === "@next" || messageId === "@previous") {
            offset = messageId === "@next" ? page.nextOffset : Math.max(0, offset - 16)
            messageOffsets.set(capture.snapshotId, offset)
            continue
          }
          try {
            const message = page.messages.find((item) => item.messageId === messageId)
            await inspect({ captureId, ...message.selection })
          } catch (error) {
            if (disposed) throw error
            context.ui.toast.show({ message: `Detail unavailable; previous selection retained if preparation failed. ${short(error.message)}`, variant: "error" })
          }
        }
      }
    } catch (error) {
      if (!disposed) context.ui.toast.show({ message: `Tracer view unavailable. ${short(error.message)}`, variant: "error" })
    } finally {
      busy = false
    }
  }
  context.keymap.layer(() => ({
    mode: "global",
    commands: [{ id: "rostfrei.presentation.spike.inspect", title: "Inspect saved Tracer evidence", palette: true, slash: { name: "tracer-evidence" }, bind: "ctrl+g", enabled: () => !busy, run }],
    bindings: ["rostfrei.presentation.spike.inspect"],
  }))
  return () => { disposed = true }
}

export default Plugin.define({ id: "rostfrei.presentation.spike.tui", setup: installInspector })
