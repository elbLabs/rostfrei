// Integration fault tests: actual adapter/server/TUI code, explicit core/host doubles.
import assert from "node:assert/strict"
import { readFile, mkdtemp, writeFile, rm } from "node:fs/promises"
import { fileURLToPath } from "node:url"
import path from "node:path"
import vm from "node:vm"
import test from "node:test"

const directory = path.dirname(fileURLToPath(import.meta.url))
async function harness() {
  const modules = new Map()
  let snapshots = 0
  const synthetic = (name, exports) => new vm.SyntheticModule(Object.keys(exports), function () {
    for (const [key, value] of Object.entries(exports)) this.setExport(key, value)
  }, { identifier: name })
  modules.set("@rostfrei/tracer-presentation", synthetic("core-double", {
    readEvidence(evidence) {
      const source = typeof evidence === "string" ? JSON.parse(evidence) : evidence
      const messages = source.messageSeries.messageSeries.messages
      return { kind: "operation", snapshotId: `test-snapshot-${++snapshots}`, source, sourceText: typeof evidence === "string" ? evidence : JSON.stringify(evidence),
        operation: source.report.operation, series: source.messageSeries,
        edges: messages.filter((m) => messages.some((p) => p.messageId === m.causationId)).map((m) => ({ parentMessageId: m.causationId, childMessageId: m.messageId })),
        captureStatus: "valid", provenance: "unknown", diagnostics: [], warnings: ["Finite capture; downstream completion unproven"],
      }
    },
    createSelection(view, messageId) { return { snapshotId: view.snapshotId, operationId: view.operation.operationId, messageId } },
    resolveSelection(view, selection) {
      assert.equal(selection.snapshotId, view.snapshotId)
      const message = view.series.messageSeries.messages.find((item) => item.messageId === selection.messageId)
      if (!message) throw new Error("Message missing")
      return { message, outcome: view.series.messageSeries.commandOutcomes.find((item) => item.commandMessageId === message.messageId), diagnostics: [], path: `/messageSeries/messageSeries/messages/${view.series.messageSeries.messages.indexOf(message)}` }
    },
    renderText() { return "bounded core double" },
  }))
  modules.set("@opencode/plugin", synthetic("server-double", { Plugin: { define: (value) => value } }))
  modules.set("@opencode/plugin/tui", synthetic("tui-double", { Plugin: { define: (value) => value } }))
  modules.set("@opencode/plugin/rpc", synthetic("rpc-double", { Rpc: { define: (value) => value } }))
  async function load(specifier, parent = path.join(directory, "entry.js")) {
    const id = specifier.startsWith(".") ? path.resolve(path.dirname(parent), specifier) : specifier
    if (modules.has(id)) return modules.get(id)
    const module = id.startsWith("node:") ? synthetic(id, await import(id)) : new vm.SourceTextModule(await readFile(id, "utf8"), { identifier: id })
    modules.set(id, module)
    if (!id.startsWith("node:")) await module.link((specifier, parent) => load(specifier, parent.identifier))
    return module
  }
  const server = await load("./server.js")
  await server.evaluate()
  const tui = await load("./tui.js")
  await tui.evaluate()
  return { server: server.namespace.default, install: tui.namespace.installInspector,
    adapter: modules.get(path.join(directory, "adapter.js")).namespace.createPresentationAdapter,
    fixture: modules.get(path.join(directory, "fixture.js")).namespace.fixture }
}
const entry = (evidence) => ({ captureId: "sha256:" + "a".repeat(64), label: "boundary fixture", evidence })
const identity = { captureId: "sha256:" + "a".repeat(64), snapshotId: "test-snapshot-1", operationId: "synthetic-operation-1", messageId: "synthetic-command-1" }

test("a callback cannot silently rebind after reopen, even for identical source and digest", async () => {
  const { adapter, fixture } = await harness()
  const first = adapter([entry(fixture)])
  first.selectMessage(identity)
  const reopened = adapter([entry(fixture)])
  assert.throws(() => reopened.selectMessage(identity), /Stale snapshot/)
  assert.equal(reopened.selectedMessage(), null)
  assert.deepEqual(first.selectedMessage(), identity)
  const fresh = { ...identity, snapshotId: reopened.listCaptures()[0].snapshotId }
  assert.deepEqual(reopened.selectMessage(fresh), fresh)
})

test("raw-number values never cross as view DTOs; summaries hide payload and bounded reveal is explicit", async () => {
  const { adapter, fixture } = await harness()
  const source = JSON.parse(JSON.stringify(fixture))
  source.messageSeries.messageSeries.messages[0].payload = { amount: JSON.rawJSON("9007199254740993"), decimal: JSON.rawJSON("0.123456789012345678901"), text: "X".repeat(1024 * 1024) }
  const view = adapter([entry(source)])
  const summary = view.inspectMessage(identity)
  assert.ok(Buffer.byteLength(JSON.stringify(summary)) < 2048)
  assert.equal(summary.payload.displayed, false)
  assert.ok(!JSON.stringify(summary).includes("9007199254740993"))
  assert.deepEqual(view.selectMessage(identity), identity)
  assert.throws(() => view.messageDetail({ ...identity, section: "payload", offset: 0, reveal: false }), /Explicit reveal/)
  const detail = view.messageDetail({ ...identity, section: "payload", offset: 0, reveal: true })
  assert.equal(detail.truncated, true)
  assert.ok(detail.text.includes("9007199254740993"))
  assert.ok(detail.text.includes("0.123456789012345678901"))
  assert.ok(detail.text.length <= 160)
  const sourcePage = view.sourcePage({ ...identity, offset: 0, reveal: true })
  assert.equal(sourcePage.text, JSON.stringify(source).slice(0, 4096))
})

test("detail preserves Unicode/control data as escaped text and labels missing payloads", async () => {
  const { adapter, fixture } = await harness()
  const source = JSON.parse(JSON.stringify(fixture))
  const payload = { text: "wide界 astral😀 \u202e\u001b[2J" }
  source.messageSeries.messageSeries.messages[0].payload = payload
  const view = adapter([entry(source)])
  const detail = view.messageDetail({ ...identity, section: "payload", offset: 0, reveal: true })
  assert.deepEqual(JSON.parse(detail.text), payload)
  assert.ok(!/[^\x20-\x7e]/.test(detail.text))
  const missing = view.messageDetail({ ...identity, messageId: "synthetic-orphan-1", section: "payload", offset: 0, reveal: true })
  assert.equal(missing.policy, "missing-or-redacted")
})

test("actual registered inspect tool is bounded even when payload is huge", async () => {
  const { server, fixture } = await harness()
  const tools = []
  const temporary = await mkdtemp(path.resolve(directory, "../../../artifacts/tmp/opencode-boundary-"))
  try {
    const doc = JSON.parse(JSON.stringify(fixture))
    doc.messageSeries.messageSeries.messages[0].payload = { text: "X".repeat(1024 * 1024) }
    const original = JSON.stringify(doc)
    const file = path.join(temporary, "capture.json")
    await writeFile(file, original)
    await server.setup({ options: { captures: [{ path: file, label: "huge fixture" }] }, tool: { transform: async (fn) => fn({ namespace() {}, add: (tool) => tools.push(tool) }) }, rpc: { register: async () => {} } })
    const { createHash } = await import("node:crypto")
    const captureId = "sha256:" + createHash("sha256").update(original).digest("hex")
    await writeFile(file, "changed after immutable load")
    const output = await tools.find((tool) => tool.name === "inspect_message").execute({ ...identity, captureId })
    assert.ok(Buffer.byteLength(output.content) < 2048)
    assert.equal(JSON.parse(output.content).payload.displayed, false)
  } finally { await rm(temporary, { recursive: true }) }
})

function deferred() { let resolve; const promise = new Promise((r) => { resolve = r }); return { promise, resolve } }
function context(client, select) {
  let command
  const toasts = []
  return { ctx: { location: { directory: "/owned" }, client: { rpc: () => client }, keymap: { layer: (fn) => { command = fn().commands[0] } },
    ui: { dialog: { select }, toast: { show: (message) => toasts.push(message) } } }, get command() { return command }, toasts }
}

test("overlapping invocation cannot replace the active dialog; guard resets after cancel", async () => {
  const { install } = await harness()
  const slow = deferred()
  let calls = 0
  let dialogs = 0
  const test = context({ listCaptures: () => (++calls === 1 ? slow.promise : Promise.resolve([])), selectedMessage: async () => null }, async () => { dialogs++; return undefined })
  install(test.ctx)
  const first = test.command.run()
  await test.command.run()
  assert.equal(calls, 1)
  slow.resolve([])
  await first
  assert.equal(dialogs, 1)
  await test.command.run()
  assert.equal(dialogs, 2)
})

test("unload invalidates a pending response without opening a stale dialog/toast", async () => {
  const { install } = await harness()
  const slow = deferred()
  const test = context({ listCaptures: () => slow.promise }, () => assert.fail("stale dialog"))
  const dispose = install(test.ctx)
  const running = test.command.run()
  dispose()
  slow.resolve([])
  await running
  assert.equal(test.toasts.length, 0)
})

test("failed detail preparation never commits selection and returns to message picker", async () => {
  const { install } = await harness()
  let committed = false
  const titles = []
  let captures = 0, messages = 0
  const test = context({
    listCaptures: async () => [{ ...identity, label: "capture", decision: "accepted" }], selectedMessage: async () => null,
    listMessages: async () => ({ captureStatus: "valid", messages: [{ messageId: identity.messageId, selection: identity, name: "rent", kind: "command" }], nextOffset: null }),
    inspectMessage: async () => { throw new Error("read failed") }, selectMessage: async () => { committed = true },
  }, async (dialog) => {
    titles.push(dialog.title)
    if (dialog.title.startsWith("Saved")) return ++captures === 1 ? identity.captureId : undefined
    return ++messages === 1 ? identity.messageId : undefined
  })
  install(test.ctx)
  await test.command.run()
  assert.equal(committed, false)
  assert.equal(titles.filter((title) => title.startsWith("Messages")).length, 2)
  assert.equal(test.toasts.length, 1)
})

test("back and reopen retain a selected message beyond the first bounded list page", async () => {
  const { install } = await harness()
  let selected = null
  const offsets = []
  const choices = [identity.captureId, "@next", identity.messageId, undefined, undefined, undefined,
    identity.captureId, undefined, undefined]
  const test = context({
    listCaptures: async () => [{ ...identity, label: "fixture", decision: "accepted" }],
    selectedMessage: async () => selected,
    listMessages: async ({ offset }) => {
      offsets.push(offset)
      return { captureStatus: "valid", messages: offset ? [{ messageId: identity.messageId, selection: identity, name: "seventeenth", kind: "command" }] : [], nextOffset: offset ? null : 16 }
    },
    inspectMessage: async () => ({ heading: "seventeenth", summary: ["Outcome: accepted"], payload: { state: "missing-or-redacted" } }),
    selectMessage: async (value) => { selected = value; return value },
  }, async (dialog) => {
    if (dialog.title.startsWith("Messages") && selected) assert.equal(dialog.current, identity.messageId)
    return choices.shift()
  })
  install(test.ctx)
  await test.command.run()
  await test.command.run()
  assert.deepEqual(offsets, [0, 16, 16, 16])
  assert.equal(choices.length, 0)
})
