import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import test from "node:test"
import { createPresentationAdapter } from "./adapter.js"
import { fixture } from "./fixture.js"
import { readFileSync } from "node:fs"

const entry = (evidence) => ({
  captureId: `sha256:${createHash("sha256").update(typeof evidence === "string" ? evidence : JSON.stringify(evidence)).digest("hex")}`,
  label: "installed adapter test", evidence,
})
const selected = (adapter, capture, index = 0) => ({ captureId: capture.captureId, ...adapter.listMessages({ ...capture, offset: 0 }).messages[index].selection })
const detail = (adapter, identity, section, reveal = false) => {
  let text = "", offset = 0
  while (true) {
    const page = adapter.messageDetail({ ...identity, section, reveal, offset })
    text += page.text
    if (page.nextOffset === null) return text
    offset = page.nextOffset
  }
}

test("same operation ID in different captures cannot alias selection", () => {
  const second = JSON.parse(JSON.stringify(fixture)) // Synthetic JSON-only fixture.
  // Deliberately synthetic second capture with a reused operation identity.
  second.messageSeries.messageSeries.messages[1].messageId = "synthetic-other-event"
  const entry = (evidence) => ({
    captureId: `sha256:${createHash("sha256").update(JSON.stringify(evidence)).digest("hex")}`,
    label: evidence.source, evidence,
  })
  const captures = [entry(fixture), entry(second)]
  const adapter = createPresentationAdapter(captures)
  const otherInstance = createPresentationAdapter(captures)
  const [firstCapture, secondCapture] = adapter.listCaptures()
  const selection = {
    captureId: captures[0].captureId,
    snapshotId: firstCapture.snapshotId,
    operationId: fixture.messageSeries.operationId,
    messageId: "synthetic-event-1",
  }
  adapter.selectMessage(selection)
  assert.throws(() => adapter.selectMessage({ ...selection, captureId: captures[1].captureId, snapshotId: secondCapture.snapshotId }))
  assert.deepEqual(adapter.selectedMessage(), selection)
  assert.equal(otherInstance.selectedMessage(), null)
  assert.throws(() => otherInstance.selectMessage(selection), /Stale snapshot/)
  assert.equal(adapter.inspectMessage({ ...selection, captureId: captures[1].captureId, snapshotId: secondCapture.snapshotId, messageId: "synthetic-other-event" }).identity.messageId, "synthetic-other-event")
  assert.equal(adapter.showOperation(selection).operationId, fixture.messageSeries.operationId)
  assert.equal(adapter.sourcePage({ ...selection, offset: 0, reveal: true }).text, JSON.stringify(fixture).slice(0, 4096))
})

test("compiled installed core reads both unchanged canonical files and preserves accepted unit/outcome forms", () => {
  for (const [name, decision, hash] of [
    ["accepted", "accepted", "d99e755cd5f92bc3741685d04a89aba9db26de3deb2eab3d219533418dff38fb"],
    ["rejected", "rejected", "3eaab73f65802130c3c80ff1cd42a98cb474832edae901391aad700e46778339"],
  ]) {
    const original = readFileSync(new URL(`./test-evidence/${name}.json`, import.meta.url), "utf8")
    const saved = entry(original)
    assert.equal(saved.captureId, "sha256:" + hash)
    const adapter = createPresentationAdapter([saved])
    const capture = adapter.listCaptures()[0]
    assert.equal(capture.captureStatus, "valid")
    const command = selected(adapter, capture)
    assert.equal(adapter.inspectMessage(command).summary[1], `Outcome: ${decision}`)
    const outcome = JSON.parse(detail(adapter, command, "outcome", true)).outcome
    assert.equal(outcome.status, decision)
    if (decision === "accepted") {
      assert.equal(Object.hasOwn(outcome, "value"), false)
      const event = selected(adapter, capture, 1)
      assert.match(adapter.inspectMessage(event).summary[1], /^Parent matched:/)
      assert.deepEqual(adapter.selectMessage(event), event)
    } else assert.equal(outcome.value.code, "BICYCLE_UNAVAILABLE")
  }
})

test("real core raw-number values travel only as exact bounded text", () => {
  const source = JSON.parse(JSON.stringify(fixture))
  const payload = { integer: JSON.rawJSON("9007199254740993"), fraction: JSON.rawJSON("0.123456789012345678901"), underflow: JSON.rawJSON("1e-999"), negativeZero: JSON.rawJSON("-0") }
  source.messageSeries.messageSeries.messages[0].payload = payload
  const original = JSON.stringify(source)
  const adapter = createPresentationAdapter([entry(original)])
  const capture = adapter.listCaptures()[0]
  const identity = selected(adapter, capture)
  assert.equal(detail(adapter, identity, "payload", true), JSON.stringify(payload))
  assert.deepEqual(adapter.selectMessage(identity), identity)
  assert.equal(adapter.inspectMessage(identity).payload.displayed, false)
  assert.equal(adapter.sourcePage({ ...capture, offset: 0, reveal: true }).text, original)
})

test("invalid finite capture retains operation facts and diagnostics but offers no selectable messages", () => {
  const source = JSON.parse(JSON.stringify(fixture))
  source.messageSeries.messageSeries.messages[0].unexpected = true
  const adapter = createPresentationAdapter([entry(JSON.stringify(source))])
  const capture = adapter.listCaptures()[0]
  assert.equal(capture.captureStatus, "invalid")
  assert.equal(adapter.showOperation(capture).decision, "accepted")
  assert.deepEqual(adapter.listMessages({ ...capture, offset: 0 }).messages, [])
  assert.throws(() => adapter.selectMessage({ ...capture, messageId: "synthetic-command-1" }), /missing or invalid/)
  let text = "", offset = 0
  while (true) {
    const page = adapter.captureDetail({ ...capture, offset })
    text += page.text
    if (page.nextOffset === null) break
    offset = page.nextOffset
  }
  assert.ok(JSON.parse(text).diagnostics.some((item) => item.code === "INVALID_CAPTURE"))
})

test("anomalous self-link stays disconnected instead of rebuilding a raw-ID edge", () => {
  const source = JSON.parse(JSON.stringify(fixture))
  const event = source.messageSeries.messageSeries.messages[1]
  event.causationId = event.messageId
  const adapter = createPresentationAdapter([entry(JSON.stringify(source))])
  const capture = adapter.listCaptures()[0]
  const identity = selected(adapter, capture, 1)
  assert.equal(capture.captureStatus, "anomalous")
  assert.equal(adapter.inspectMessage(identity).summary[1], "No validated parent edge; disconnected")
  assert.ok(JSON.parse(detail(adapter, identity, "limitations")).diagnostics.some((item) => item.code === "CAUSATION_CYCLE"))
})
