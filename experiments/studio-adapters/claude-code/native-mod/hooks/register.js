// snapshots.js is generated in an owned artifact directory by run-mod.mjs.
// All evidence interpretation and text come from the shared presentation core.
import { snapshots } from '../snapshots.js'

function freeze(value) {
  if (value && typeof value === 'object') {
    Object.values(value).forEach(freeze)
    Object.freeze(value)
  }
  return value
}
const captures = freeze(snapshots)
let active = captures[0]
let selected = null

function captureKey(capture) {
  return JSON.stringify([capture.source, capture.captureId])
}

function choose(selection) {
  if (selection?.captureId !== active.captureId || selection?.source !== active.source) {
    throw new Error('Selection belongs to another saved capture')
  }
  if (selection.snapshotId !== active.snapshotId) throw new Error('Selection belongs to another open snapshot')
  if (selection.operationId !== active.operationId) throw new Error('Operation mismatch')
  const message = active.messages.find((item) => item.messageId === selection.messageId)
  if (!message) throw new Error('Message missing from this saved capture')
  selected = message
  return identity()
}

function identity() {
  return selected ? {
    captureId: active.captureId, source: active.source,
    ...selected.selection,
  } : null
}

export function register(on) {
  on('session.start', async ($, e, next) => {
    await $.command.register({ name: 'studio-pane', description: 'Open saved Tracer evidence' })
    await $.command.register({ name: 'studio-selection', description: 'Return the selected saved-capture identity' })
    await $.command.register({ name: 'studio-select', description: 'Inspect an identity within the active saved capture', argumentHint: '<selection JSON>' })
    return next(e)
  })

  on('command.run', { command: 'studio-pane' }, async ($) => {
    await $.ui.open({ id: 'rostfrei-studio-pane', title: 'Saved Tracer evidence', focus: true, closeOnEscape: true })
    return {}
  })

  on('command.run', { command: 'studio-selection' }, async () => ({ text: JSON.stringify(identity()) }))

  on('command.run', { command: 'studio-select' }, async ($, e) => {
    try {
      const selection = choose(JSON.parse(e.args))
      $.ui.invalidate('ui.render')
      return { text: JSON.stringify(selection) }
    } catch (error) {
      return { text: 'Selection rejected: ' + error.message }
    }
  })

  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== 'rostfrei-studio-pane') return next(e)
    const drawn = active // Bind callbacks to the view that drew them, never a later active view.
    const { Box, Text, Select } = $.ui.resolve(e)
    return Box({ flexDirection: 'column', children: [
      Text({ children: [active.label + '\n' + active.source + '\n' + active.captureId] }),
      Select({
        key: 'capture', label: 'Saved capture', value: captureKey(active),
        options: captures.map((capture) => ({ value: captureKey(capture), label: capture.label })),
        onSelect: (value) => {
          const capture = captures.find((item) => captureKey(item) === value)
          if (!capture) return
          if (capture !== active) selected = null
          active = capture
          $.ui.invalidate('ui.render')
        },
      }),
      Text({ children: [active.summary] }),
      Select({
        key: 'message', label: 'Inspect message',
        ...(selected ? { value: selected.messageId } : {}),
        options: active.messages.map((message, index) => ({ value: message.messageId, label: 'Message ' + (index + 1) })),
        onSelect: (messageId) => {
          const message = drawn.messages.find((item) => item.messageId === messageId)
          choose({ captureId: drawn.captureId, source: drawn.source, ...message?.selection })
          $.ui.invalidate('ui.render')
        },
      }),
      Text({ children: [selected ? selected.details : 'Choose a message to inspect.'] }),
    ] })
  })
}
