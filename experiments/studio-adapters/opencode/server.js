import { Plugin } from "@opencode/plugin"
import { open } from "node:fs/promises"
import { createHash } from "node:crypto"
import { createPresentationAdapter } from "./adapter.js"
import { fixture } from "./fixture.js"
import { Presentation, operationInput, messageInput, emptyInput } from "./rpc.js"

export default Plugin.define({
  id: "rostfrei.presentation.spike",
  async setup(ctx) {
    if (ctx.options.captures?.length > 16) throw new Error("Load at most 16 captures")
    const captures = Array.isArray(ctx.options.captures) && ctx.options.captures.length
      ? await Promise.all(ctx.options.captures.map(async ({ path, label }) => {
        if (typeof path !== "string" || !path.startsWith("/")) throw new Error("Expected absolute saved-evidence path")
        const file = await open(path, "r")
        let bytes
        try {
          bytes = Buffer.alloc(8 * 1024 * 1024 + 1)
          let length = 0
          while (length < bytes.length) {
            const chunk = await file.read(bytes, length, bytes.length - length, null)
            if (!chunk.bytesRead) break
            length += chunk.bytesRead
          }
          if (length === bytes.length) throw new Error("Saved capture exceeds 8 MiB limit")
          bytes = bytes.subarray(0, length)
        } finally { await file.close() }
        return { captureId: `sha256:${createHash("sha256").update(bytes).digest("hex")}`, label, evidence: new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes) }
      }))
      : [{
        captureId: `sha256:${createHash("sha256").update(JSON.stringify(fixture)).digest("hex")}`,
        label: fixture.source, evidence: fixture,
      }]
    const adapter = createPresentationAdapter(captures)
    await ctx.tool.transform((editor) => {
      editor.namespace({ name: "rostfrei_presentation", description: "Read loaded saved Tracer captures in one owned instance; no execution" })
      editor.add({
        name: "show_operation",
        description: "Read a saved operation by capture digest, snapshot and operation ID, with capture limitations",
        input: operationInput,
        options: { namespace: "rostfrei_presentation", codemode: true },
        execute: async (input) => ({ content: adapter.renderOperation(input) }),
      })
      editor.add({
        name: "inspect_message",
        description: "Bounded selected-message summary; payloads hidden; capture digest required",
        input: messageInput,
        options: { namespace: "rostfrei_presentation", codemode: true },
        execute: async (input) => ({ content: JSON.stringify(adapter.inspectMessage(input)) }),
      })
      editor.add({
        name: "selected_message",
        description: "Read the digest-bound selection in this single owned instance (not session scoped), or null",
        input: emptyInput,
        options: { namespace: "rostfrei_presentation", codemode: true },
        execute: async () => ({ content: JSON.stringify(adapter.selectedMessage()) }),
      })
    })
    const checked = (read) => async (input, context) => {
      try {
        return read(input)
      } catch (error) {
        return context.error("invalid_selection", error.message, { reason: error.message })
      }
    }
    await ctx.rpc.register(Presentation, {
      listCaptures: async () => adapter.listCaptures(),
      showOperation: checked(adapter.showOperation),
      listMessages: checked(adapter.listMessages),
      captureDetail: checked(adapter.captureDetail),
      renderOperation: checked(adapter.renderOperation),
      inspectMessage: checked(adapter.inspectMessage),
      messageDetail: checked(adapter.messageDetail),
      sourcePage: checked(adapter.sourcePage),
      selectMessage: checked(adapter.selectMessage),
      selectedMessage: async () => adapter.selectedMessage(),
      diagnostics: async () => ({
        app: ctx.app,
        tools: (await ctx.tool.list()).filter((tool) => tool.id.startsWith("rostfrei_presentation_")).map((tool) => ({ id: tool.id, input: tool.input })),
        selectionScope: "single-owned-plugin-instance; not session-safe",
        captures: adapter.listCaptures(),
      }),
    })
  },
})
