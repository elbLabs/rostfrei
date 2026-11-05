import { App } from '@modelcontextprotocol/ext-apps'

type Binding = { viewId: string; sha256: string; operationId: string | null }
type Selection = { operationId: string; messageId: string }
type Message = { messageId: string; label: string; detailText: string }
type Handoff = { binding: Binding; evidenceJson: string; selection: Selection | null; presentation: { summary: string; messages: Message[] } }
const app = new App({ name: 'rostfrei saved evidence', version: '0.0.1' }, {})
const element = (id: string) => document.getElementById(id)!
const say = (text: string) => {
  for (const id of ['status', 'sharing-status', 'detail-status', 'plain-status']) element(id).textContent = text
}
let handoff: Handoff | undefined
let selection: Selection | null = null
let confirmedSelection: Selection | null = null
let revision = 0
let queue = Promise.resolve()
let selectedButton: HTMLButtonElement | undefined
let plain = false

function sharing(state: string, text: string) {
  element('reader').dataset.sharingState = state
  say(`${text}\nLocal highlight: ${selection?.messageId ?? 'none'}.\nLast server-confirmed selection: ${confirmedSelection?.messageId ?? 'none'}.`)
}

function renderDetails() {
  if (!handoff) return
  const detail = handoff.presentation.messages.find((message) => message.messageId === selection?.messageId)?.detailText
  element('details').textContent = detail ?? ''
  element('empty').hidden = !!selection
  element('back').hidden = !selection
  element('text').textContent = detail ?? handoff.presentation.summary
  document.querySelectorAll<HTMLButtonElement>('[data-message-id]').forEach((button) => button.setAttribute('aria-pressed', String(button.dataset.messageId === selection?.messageId)))
}

function choose(next: Selection | null) {
  if (!handoff) return
  if (next && !handoff.presentation.messages.some((message) => message.messageId === next.messageId)) return
  selection = next
  renderDetails()
  if (next && matchMedia('(max-width:640px)').matches) {
    element('details-title').focus()
    element('details-title').scrollIntoView({ block: 'start' })
  }
  const intent = ++revision
  const bound = handoff.binding
  if (!app.getHostCapabilities()?.serverTools) {
    sharing('local', 'Inspecting locally — host has no server-tools return path. Name the message ID in conversation.')
    return
  }
  sharing('pending', 'Inspecting locally — sharing selection…')
  // Serialize writes; local inspection stays immediate. An obsolete frame cannot update the server.
  queue = queue.then(async () => {
    if (handoff?.binding.viewId !== bound.viewId) return
    const current = () => intent === revision && handoff?.binding.viewId === bound.viewId
    let acknowledged = false
    try {
      const updated = await app.callServerTool({ name: 'select_message', arguments: { binding: bound, messageId: next?.messageId ?? null } }, { timeout: 10000 })
      if (updated.isError) {
        if (current()) sharing('refused', 'Selection write refused. Server state was not confirmed again. Reopen to check.')
        return
      }
      acknowledged = true
      if (current()) sharing('acknowledged', 'Selection write acknowledged; readback pending.')
      const readback = await app.callServerTool({ name: 'get_selection', arguments: { binding: bound } }, { timeout: 10000 })
      const state = readback.structuredContent as { binding?: Binding; selection?: Selection | null } | undefined
      if (readback.isError || state?.binding?.viewId !== bound.viewId || state.binding.sha256 !== bound.sha256 || state.binding.operationId !== bound.operationId || (next ? state.selection?.messageId !== next.messageId || state.selection.operationId !== next.operationId : state.selection !== null)) throw new Error('Readback mismatch')
      if (handoff?.binding.viewId !== bound.viewId) return
      confirmedSelection = state.selection!
      if (current()) sharing('confirmed', next ? 'Selected message confirmed by this server connection. Model consumption has not been tested.' : 'Selection cleared and confirmed by this server connection.')
    } catch {
      if (current()) sharing('unknown', `${acknowledged ? 'Selection write acknowledged; readback confirmation unavailable.' : 'Selection write result unknown.'} Server selection may have changed. Reopen to check; no rerun is needed.`)
    }
  })
}

app.ontoolresult = (result) => {
  try {
    const incoming = result.structuredContent as Handoff | undefined
    // App-initiated selection replies are not new evidence handoffs.
    if (!incoming?.evidenceJson) return
    if (!incoming.binding?.viewId || !/^[a-f0-9]{64}$/.test(incoming.binding.sha256) || typeof incoming.presentation?.summary !== 'string' || !Array.isArray(incoming.presentation.messages)) throw new Error('Invalid presentation handoff')
    if (incoming.selection && (incoming.selection.operationId !== incoming.binding.operationId || !incoming.presentation.messages.some((message) => message.messageId === incoming.selection?.messageId))) throw new Error('Invalid selection handoff')
    handoff = incoming
    selection = incoming.selection
    confirmedSelection = incoming.selection
    revision++
    element('reader').hidden = false
    element('status').classList.add('sr-only')
    element('binding').textContent = `Capture SHA-256: ${incoming.binding.sha256}\nView: ${incoming.binding.viewId}`
    element('summary').textContent = incoming.presentation.summary
    element('source').textContent = incoming.evidenceJson
    element('messages').replaceChildren()
    selectedButton = undefined
    for (const message of incoming.presentation.messages) {
      const button = document.createElement('button')
      button.type = 'button'
      button.dataset.messageId = message.messageId
      button.append(document.createTextNode(message.label))
      const id = document.createElement('small')
      id.textContent = message.messageId
      button.append(id)
      button.onclick = () => { selectedButton = button; choose({ operationId: incoming.binding.operationId!, messageId: message.messageId }) }
      if (message.messageId === selection?.messageId) selectedButton = button
      const li = document.createElement('li')
      li.append(button)
      element('messages').append(li)
    }
    if (!element('messages').childElementCount) element('messages').textContent = 'No finite captured messages available.'
    renderDetails()
    sharing('confirmed', app.getHostCapabilities()?.serverTools ? 'Saved evidence ready. Server selection received; choose a message to inspect and check sharing.' : 'Saved evidence ready. Local inspection only; name IDs in conversation.')
  } catch { say('Evidence display unavailable. Use the original text tool result; no rerun is needed.') }
}
app.onhostcontextchanged = (context) => {
  if (context.theme === 'light' || context.theme === 'dark') document.documentElement.dataset.theme = context.theme
}
element('clear').onclick = () => choose(null)
element('clear-details').onclick = () => choose(null)
element('back').onclick = () => selectedButton?.focus()
element('display').onclick = () => {
  plain = !plain
  element('plain').hidden = !plain
  element('visual').hidden = plain
  element('display').textContent = plain ? 'Show message reader' : 'Show plain text'
}
try {
  await app.connect()
  const context = app.getHostContext()
  if (context?.theme) document.documentElement.dataset.theme = context.theme
} catch {
  say('Host connection unavailable. Read the original text tool result; no rerun is needed.')
}
