import { App, applyDocumentTheme, applyHostStyleVariables, type McpUiHostContext } from '@modelcontextprotocol/ext-apps'
import { createRoot } from 'react-dom/client'
import { flushSync } from 'react-dom'
import { EvidenceSummary, EvidenceDisclosure, EvidenceMessageList, EvidenceFlow } from '@rostfrei/ui/evidence'
import type { EvidenceSummary as Summary } from '../../../../packages/tracer-presentation/src/index.ts'
import '@rostfrei/ui/tokens.css'
import '@rostfrei/ui/evidence.css'
import './view.css'
import signika from '../../../../packages/ui/src/fonts/signika-negative-latin-variable.woff2'
import jetbrains from '../../../../packages/ui/src/fonts/jetbrains-mono-regular.woff2'

// The host forbids font URLs. Bundled bytes need no resource fetch or CSP grant.
for (const face of [new FontFace('Signika Negative', signika, { weight: '300 700', display: 'swap' }), new FontFace('JetBrains Mono', jetbrains, { display: 'swap' })]) {
  document.fonts.add(face)
  void face.load().catch(() => {})
}

type Binding = { viewId: string; sha256: string; operationId: string | null }
type Selection = { operationId: string; messageId: string }
type Message = { id: string; kind: 'command' | 'domain-event' | 'integration-event'; name: string; detail: string; detailText: string }
type Handoff = { binding: Binding; evidenceJson: string; selection: Selection | null; presentation: { summary: Summary; text: string; mermaid: string | null; flowNote: string; messages: Message[]; edges: { from: string; to: string }[] } }
const app = new App({ name: 'rostfrei saved evidence', version: '0.0.1' }, {})
const root = createRoot(document.getElementById('root')!)
let handoff: Handoff | undefined
let selection: Selection | null = null
let confirmedSelection: Selection | null = null
let revision = 0
let queue = Promise.resolve()
let plain = false
let detailsOpen = false
let sharingState = 'connecting'
let feedback = 'Connecting to the host… Text remains available in the tool result.'

function messageButton(id: string) {
  return [...document.querySelectorAll<HTMLElement>('[data-evidence-message]')].find((element) => element.dataset.evidenceMessage === id)?.querySelector('button')
}

function Reader() {
  if (!handoff) return <main><p id="status" role="status" aria-live="polite">{feedback}</p></main>
  const { summary, messages, text, mermaid, flowNote, edges } = handoff.presentation
  const flowMessages = messages.map((message) => ({ ...message, tone: message.kind }))
  const selectedMessage = messages.find((message) => message.id === selection?.messageId)
  const detail = selectedMessage?.detailText
  const select = (messageId: string) => choose({ operationId: handoff!.binding.operationId!, messageId })
  return <main id="reader" data-sharing-state={sharingState}>
    <div id="summary"><EvidenceSummary title={summary.title} facts={summary.facts.filter((fact) => ['Command outcome', 'Test verdict', 'Capture'].includes(fact.label))}>
      {summary.limitations.length === 1 ? <p className="limitations">{summary.limitations[0]}</p> : <ul className="limitations">{summary.limitations.map((line, index) => <li key={index}>{line}</li>)}</ul>}
      {summary.issues.map((issue, index) => <p className="diagnostic-notice" key={index}>{issue}</p>)}
    </EvidenceSummary></div>
    <p id="status" className="sr-only" role="status" aria-live="polite">{feedback}</p>
    <div className="actions"><button id="display" onClick={() => { plain = !plain; render() }}>{plain ? 'Flow view' : 'Text view'}</button><button id="clear" hidden={!selection} onClick={() => choose(null)}>Clear selection</button></div>
    <div id="visual" hidden={plain}>
      <section aria-labelledby="messages-title"><h2 id="messages-title">Captured messages</h2>
        <div id="messages"><EvidenceFlow messages={flowMessages.slice(0, 12)} edges={edges} selectedId={selection?.messageId} onSelect={select} /></div>
        {!messages.length && !flowNote && <p>No messages recorded.</p>}
        {messages.length > 12 && <EvidenceDisclosure title={`More captured messages (${messages.length - 12})`}><EvidenceMessageList messages={flowMessages.slice(12)} selectedId={selection?.messageId} onSelect={select} /></EvidenceDisclosure>}
        {flowNote && <p className="muted">{flowNote}</p>}
      </section>
      <EvidenceDisclosure title="Message details" defaultOpen={detailsOpen}>
        <section aria-labelledby="details-title">
          <h2 id="details-title" tabIndex={-1} aria-describedby="detail-status">{selectedMessage?.name ?? 'No message selected'}</h2>
          <p id="detail-status" className="selection-status">{feedback}</p>
          <button id="clear-details" onClick={() => choose(null)}>Clear selection</button>
          <p id="empty" hidden={!!selection}>Choose a message to inspect it.</p>
          <pre id="details">{detail ?? ''}</pre>
          <button id="back" hidden={!selection} onClick={() => selection && messageButton(selection.messageId)?.focus()}>Back to selected message</button>
        </section>
      </EvidenceDisclosure>
    </div>
    <section id="plain" hidden={!plain} aria-label="Plain text evidence"><p id="plain-status" className="selection-status">{feedback}</p><pre id="text">{detail ?? text}</pre></section>
    <EvidenceDisclosure title="Evidence details">
      <p className="muted">Read-only saved capture. Connections show recorded causation.</p>
      <p id="binding" className="muted">{`Capture SHA-256: ${handoff.binding.sha256}\nView: ${handoff.binding.viewId}`}</p>
      <p className="muted">The digest identifies file bytes, not authenticity or scope.</p>
      <pre>{text}</pre>
      <EvidenceDisclosure title="Mermaid source"><pre id="mermaid">{mermaid ?? flowNote}</pre></EvidenceDisclosure>
      <EvidenceDisclosure title="Original JSON"><pre id="source">{handoff.evidenceJson}</pre></EvidenceDisclosure>
    </EvidenceDisclosure>
  </main>
}
function render() { flushSync(() => root.render(<Reader />)) }
function say(text: string) { feedback = text; render() }
function sharing(state: string, text: string) {
  sharingState = state
  say(`${text}\nLocal highlight: ${selection?.messageId ?? 'none'}.\nLast server-confirmed selection: ${confirmedSelection?.messageId ?? 'none'}.`)
}

function choose(next: Selection | null) {
  if (!handoff || (next && !handoff.presentation.messages.some((message) => message.id === next.messageId))) return
  selection = next
  detailsOpen = true
  render()
  const disclosure = document.getElementById('details-title')?.closest('details')
  if (disclosure) disclosure.open = true
  if (next && matchMedia('(max-width:640px)').matches) {
    document.getElementById('details-title')?.focus()
    document.getElementById('details-title')?.scrollIntoView({ block: 'start' })
  }
  const intent = ++revision
  const bound = handoff.binding
  if (!app.getHostCapabilities()?.serverTools) {
    sharing('local', 'Inspecting locally — host has no server-tools return path. Name the message ID in conversation.')
    return
  }
  sharing('pending', 'Inspecting locally — sharing selection…')
  // The connection owns serialized writes. Old frames never rebind to new views.
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
    if (!incoming?.evidenceJson) return
    if (!incoming.binding?.viewId || !/^[a-f0-9]{64}$/.test(incoming.binding.sha256) || typeof incoming.presentation?.summary?.title !== 'string' || !Array.isArray(incoming.presentation.summary.facts) || !Array.isArray(incoming.presentation.summary.issues) || !Array.isArray(incoming.presentation.messages)) throw new Error('Invalid presentation handoff')
    if (incoming.selection && (incoming.selection.operationId !== incoming.binding.operationId || !incoming.presentation.messages.some((message) => message.id === incoming.selection?.messageId))) throw new Error('Invalid selection handoff')
    handoff = incoming
    selection = incoming.selection
    confirmedSelection = incoming.selection
    detailsOpen = !!selection
    revision++
    sharing('confirmed', app.getHostCapabilities()?.serverTools ? 'Saved evidence ready. Server selection received; choose a message to inspect and check sharing.' : 'Saved evidence ready. Local inspection only; name IDs in conversation.')
  } catch { say('Evidence display unavailable. Use the original text tool result; no rerun is needed.') }
}
function theme(context: McpUiHostContext) {
  if (context.theme) {
    applyDocumentTheme(context.theme)
    document.documentElement.classList.toggle('dark', context.theme === 'dark')
    document.documentElement.classList.toggle('light', context.theme === 'light')
  }
  if (context.styles?.variables) {
    applyHostStyleVariables(context.styles.variables)
    for (const [token, host] of Object.entries({ '--background': '--color-background-primary', '--foreground': '--color-text-primary', '--muted-foreground': '--color-text-secondary', '--font-sans': '--font-sans' })) {
      if (token !== host && Object.hasOwn(context.styles.variables, host)) document.documentElement.style.setProperty(token, `var(${host})`)
    }
  }
}
app.onhostcontextchanged = theme
render()
try {
  await app.connect()
  const context = app.getHostContext()
  if (context) theme(context)
} catch { say('Host connection unavailable. Read the original text tool result; no rerun is needed.') }
