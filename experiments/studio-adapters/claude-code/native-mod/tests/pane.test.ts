import { expect, test } from 'claude-code/testing'
import { snapshots } from '../snapshots.js'

const PANE = {
  plugin: 'rostfrei-studio-pane', component: 'Pane', requestId: 'rostfrei-studio-pane',
  viewport: { columns: 100, rows: 30 },
  props: { title: 'Saved Tracer evidence', isFocused: true, bodyColumns: 60,
    placement: 'inline', scroll: { offset: 0, bodyRows: 10 }, view: {} },
} as const

function setup(on) {
  const commands: string[] = []
  on('session.start', () => ({ cwd: '/owned-test' }))
  on('command.register', ($, e) => {
    commands.push(e.name)
    return { value: undefined }
  })
  on('ui.open', ($, e) => {
    expect(e.id).toBe('rostfrei-studio-pane')
    return { value: { isPlaced: true } }
  })
  return commands
}

function identity(capture, message) {
  return { captureId: capture.captureId, source: capture.source,
    snapshotId: capture.snapshotId, operationId: capture.operationId, messageId: message.messageId }
}

function captureKey(capture) {
  return JSON.stringify([capture.source, capture.captureId])
}

test('saved operations and every message return exact capture-bound identities through native events', async ($, on) => {
  const commands = setup(on)
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/owned-test' })
  expect(commands).toEqual(['studio-pane', 'studio-selection', 'studio-select'])
  await $.command.run({ command: 'studio-pane', args: '' })
  // The native runner validates element trees; it does not paint either app.
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ ...PANE, surface })
    for (const capture of snapshots) {
      await ui.select({ key: 'capture', value: captureKey(capture) })
      expect(await ui.find({ type: 'Text', text: capture.summary })).toBeDefined()
      for (const message of capture.messages) {
        await ui.select({ key: 'message', value: message.messageId })
        expect(await ui.find({ type: 'Text', text: message.details })).toBeDefined()
        const returned = await $.command.run({ command: 'studio-selection', args: '' })
        expect(JSON.parse(returned.text)).toEqual(identity(capture, message))
        console.log(JSON.stringify({ event: 'ui.select -> command.run', surface,
          returned: JSON.parse(returned.text) }))
      }
    }
    await ui.unmount()
  }
  // Reopening a pane keeps the current selection within this module lifetime.
  const before = await $.command.run({ command: 'studio-selection', args: '' })
  await $.command.run({ command: 'studio-pane', args: '' })
  const reopened = await $.ui.mount({ ...PANE, surface: 'terminal' })
  const last = snapshots[snapshots.length - 1]
  expect(await reopened.find({ type: 'Text', text: last.messages[last.messages.length - 1].details })).toBeDefined()
  expect(await $.command.run({ command: 'studio-selection', args: '' })).toEqual(before)
  await reopened.unmount()
})

test('synthetic invalid and stale selections cannot cross saved captures with repeated operation/message IDs', async ($, on) => {
  setup(on)
  const [first, second] = snapshots.filter((capture) => capture.synthetic)
  expect(first.operationId).toBe(second.operationId)
  expect(first.messages[0].messageId).toBe(second.messages[0].messageId)
  expect(first.captureId).not.toBe(second.captureId)
  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' })
  await ui.select({ key: 'capture', value: captureKey(first) })
  await ui.select({ key: 'message', value: first.messages[0].messageId })
  const selected = identity(first, first.messages[0])
  for (const candidate of [
    '{invalid JSON', 'null',
    JSON.stringify({ ...selected, snapshotId: 'stale-open-snapshot' }),
    JSON.stringify({ ...selected, operationId: 'wrong-operation' }),
    JSON.stringify({ ...selected, messageId: 'missing-message' }),
    JSON.stringify({ ...selected, source: second.source }),
    JSON.stringify({ ...selected, captureId: second.captureId }),
  ]) {
    const rejected = await $.command.run({ command: 'studio-select', args: candidate })
    expect(rejected.text).toMatch(/^Selection rejected:/)
    expect(JSON.parse((await $.command.run({ command: 'studio-selection', args: '' })).text)).toEqual(selected)
    console.log(JSON.stringify({ event: 'command.run rejected', input: candidate, returned: rejected.text }))
  }
  await ui.select({ key: 'capture', value: captureKey(second) })
  expect((await $.command.run({ command: 'studio-selection', args: '' })).text).toBe('null')
  expect((await $.command.run({ command: 'studio-select', args: JSON.stringify(selected) })).text)
    .toBe('Selection rejected: Selection belongs to another saved capture')
  const secondIdentity = identity(second, second.messages[0])
  expect(JSON.parse((await $.command.run({ command: 'studio-select', args: JSON.stringify(secondIdentity) })).text))
    .toEqual(secondIdentity)
  expect(await ui.find({ type: 'Text', text: second.messages[0].details })).toBeDefined()
  await ui.unmount()
})

test('identical bytes at distinct sources remain independently selectable', async ($, on) => {
  setup(on)
  const [first, , copy] = snapshots.filter((capture) => capture.synthetic)
  expect(first.captureId).toBe(copy.captureId)
  expect(first.source).not.toBe(copy.source)
  expect(first.snapshotId).not.toBe(copy.snapshotId)
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ ...PANE, surface })
    for (const capture of [first, copy]) {
      await ui.select({ key: 'capture', value: captureKey(capture) })
      expect((await $.command.run({ command: 'studio-selection', args: '' })).text).toBe('null')
      await ui.select({ key: 'message', value: capture.messages[0].messageId })
      expect(JSON.parse((await $.command.run({ command: 'studio-selection', args: '' })).text))
        .toEqual(identity(capture, capture.messages[0]))
    }
    expect((await $.command.run({ command: 'studio-select', args: JSON.stringify(identity(first, first.messages[0])) })).text)
      .toBe('Selection rejected: Selection belongs to another saved capture')
    await ui.unmount()
  }
})
