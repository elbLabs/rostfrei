import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdir, mkdtemp, readFile, writeFile, symlink } from 'node:fs/promises'
import { spawn, execFileSync } from 'node:child_process'
import { once } from 'node:events'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Client } from '@modelcontextprotocol/client'
import { StdioClientTransport } from '@modelcontextprotocol/client/stdio'

const root = fileURLToPath(new URL('../../../../', import.meta.url))
assert.ok(process.env.EVIDENCE_DIR, 'Set EVIDENCE_DIR to a directory containing both saved tutorial captures')
const evidenceDir = resolve(process.env.EVIDENCE_DIR)
const artifactRoot = resolve(process.env.ARTIFACT_DIR ?? `${root}/artifacts/studio-adapters/mcp-apps`)
await mkdir(artifactRoot, { recursive: true })
const scratch = await mkdtemp(`${artifactRoot}/protocol-`)
const entry = resolve(process.env.SERVER_ENTRY ?? fileURLToPath(new URL('../dist/server.js', import.meta.url)))
const accepted = `${evidenceDir}/rent-available-bicycle.json`
const rejected = `${evidenceDir}/reject-unavailable-bicycle.json`
const mime = 'text/html;profile=mcp-app'
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex')
const evidenceHashes = Object.fromEntries(await Promise.all([accepted, rejected].map(async (path) => [path, hash(await readFile(path))])))
const transcripts = []

async function connect(t, evidence = rejected, capabilities = { extensions: { 'io.modelcontextprotocol/ui': { mimeTypes: [mime] } } }) {
  const transport = new StdioClientTransport({ command: process.execPath, args: [entry, '--evidence', evidence], cwd: scratch, env: {}, stderr: 'pipe' })
  const stderr = []
  transport.stderr.on('data', (data) => stderr.push(data.toString()))
  const client = new Client({ name: 'saved-evidence-protocol-test', version: '1' }, { capabilities, versionNegotiation: { mode: 'legacy' } })
  const record = { evidence, capabilities, requests: [], stderr }
  transcripts.push(record)
  t.after(async () => { await client.close(); assert.equal(stderr.join(''), '') })
  try { await client.connect(transport) } catch (error) { throw new Error(`Connect failed: ${stderr.join('')}`, { cause: error }) }
  record.server = client.getServerVersion()
  record.serverCapabilities = client.getServerCapabilities()
  const call = async (name, args = {}) => {
    const result = await client.callTool({ name, arguments: args })
    record.requests.push({ name, arguments: args, result })
    if (result.structuredContent?.evidenceJson) assert.equal(hash(result.structuredContent.evidenceJson), result.structuredContent.binding.sha256, 'Every accepted handoff reconstructs its byte digest')
    return result
  }
  return { client, call }
}

test('real stdio negotiates Apps MIME, reads self-contained resource and retains useful text', async (t) => {
  const { client, call } = await connect(t)
  const list = await client.listTools()
  assert.deepEqual(list.tools.map((tool) => tool.name).sort(), ['get_selection', 'inspect_message', 'select_message', 'show_operation'])
  const uri = list.tools.find((tool) => tool.name === 'show_operation')._meta.ui.resourceUri
  const resource = await client.readResource({ uri })
  assert.equal(resource.contents[0].mimeType, mime)
  assert.match(resource.contents[0].text, /Saved Tracer evidence/)
  assert.doesNotMatch(resource.contents[0].text, /<script[^>]+src=|<link[^>]+href=/)
  assert.deepEqual(resource.contents[0]._meta.ui.csp.connectDomains, [])
  const shown = await call('show_operation')
  assert.match(shown.content[0].text, /rejected/)
  assert.match(shown.content[0].text, /passed/)
  assert.equal(shown.structuredContent.evidenceJson, await readFile(rejected, 'utf8'))
  assert.equal(shown.structuredContent.binding.sha256, evidenceHashes[rejected])
  await assert.rejects(client.readResource({ uri: 'ui://rostfrei/synthetic-missing.html' }))
  assert.match((await call('show_operation')).content[0].text, /BICYCLE_UNAVAILABLE/, 'A failed resource read does not remove useful text')
})

for (const capabilities of [{}, { extensions: { 'io.modelcontextprotocol/ui': { mimeTypes: ['text/html'] } } }]) {
  test(`text fallback without compatible MIME: ${JSON.stringify(capabilities)}`, async (t) => {
    const { client, call } = await connect(t, rejected, capabilities)
    assert.equal((await client.listTools()).tools.find((tool) => tool.name === 'show_operation')._meta?.ui, undefined)
    assert.deepEqual((await client.listResources()).resources, [])
    const result = await call('show_operation')
    assert.match(result.content[0].text, /Text fallback/)
    assert.match(result.content[0].text, /rejected/)
    assert.match(result.content[0].text, /BICYCLE_UNAVAILABLE/)
  })
}

test('select / inspect / readback / clear, new view invalidation, exact capture and connection binding', async (t) => {
  const a = await connect(t)
  const b = await connect(t, rejected)
  const shown = (await a.call('show_operation')).structuredContent
  const other = (await b.call('show_operation')).structuredContent
  // Equal capture bytes in separate connections still require separate view bindings.
  assert.equal(shown.binding.operationId, other.binding.operationId)
  assert.equal(shown.binding.sha256, other.binding.sha256)
  assert.notEqual(shown.binding.sha256, evidenceHashes[accepted])
  const messageId = JSON.parse(shown.evidenceJson).messageSeries.messageSeries.messages[0].messageId
  const args = { binding: shown.binding, messageId }
  assert.equal((await a.call('select_message', args)).isError, undefined)
  assert.equal((await a.call('get_selection', { binding: shown.binding })).structuredContent.selection.messageId, messageId)
  assert.match((await a.call('inspect_message', args)).content[0].text, /bike-99/)
  assert.equal((await b.call('select_message', args)).isError, true)
  const reused = JSON.parse(await readFile(rejected, 'utf8'))
  reused.messageSeries.messageSeries.messages[0].payload.synthetic_capture = 'same operation and message IDs, different immutable bytes'
  const reusedPath = `${scratch}/synthetic-reused-identities.json`
  await writeFile(reusedPath, JSON.stringify(reused))
  const c = await connect(t, reusedPath)
  const third = (await c.call('show_operation')).structuredContent
  assert.equal(third.binding.operationId, shown.binding.operationId)
  assert.notEqual(third.binding.sha256, shown.binding.sha256)
  assert.equal((await c.call('select_message', args)).isError, true)
  assert.equal((await a.call('select_message', { ...args, binding: { ...shown.binding, sha256: evidenceHashes[accepted] } })).isError, true)
  for (const invalid of ['unknown', 'demo-bike-42-added', 'subject']) assert.equal((await a.call('select_message', { ...args, messageId: invalid })).isError, true)
  const reopened = (await a.call('show_operation')).structuredContent
  assert.equal(reopened.selection.messageId, messageId)
  assert.notEqual(reopened.binding.viewId, shown.binding.viewId)
  assert.equal((await a.call('select_message', args)).isError, true)
  assert.equal((await a.call('select_message', { binding: reopened.binding, messageId: null })).structuredContent.selection, null)
  const restart = await connect(t)
  assert.equal((await restart.call('show_operation')).structuredContent.selection, null)
})

test('loaded bytes remain immutable after owner file changes; no arbitrary path tool parameter', async (t) => {
  const path = `${scratch}/mutable-owner-copy.json`
  await writeFile(path, await readFile(rejected))
  const { call } = await connect(t, path)
  const first = (await call('show_operation')).structuredContent
  await writeFile(path, await readFile(accepted))
  const second = (await call('show_operation')).structuredContent
  assert.equal(first.evidenceJson, second.evidenceJson)
  assert.equal(first.binding.sha256, second.binding.sha256)
  assert.equal((await call('show_operation', { path: rejected })).isError, true)
})

test('synthetic incomplete saved evidence stays incomplete; no messages invented', async (t) => {
  const original = JSON.parse(await readFile(rejected, 'utf8'))
  delete original.messageSeries
  original.captureState = 'incomplete'
  original.error = 'SYNTHETIC capture failure'
  const path = `${scratch}/synthetic-incomplete.json`
  await writeFile(path, JSON.stringify(original))
  const { call } = await connect(t, path)
  const result = await call('show_operation')
  assert.match(result.content[0].text, /incomplete/)
  const messageId = JSON.parse(await readFile(rejected, 'utf8')).messageSeries.messageSeries.messages[0].messageId
  assert.equal((await call('select_message', { binding: result.structuredContent.binding, messageId })).isError, true)
})

test('synthetic hostile labels and unsafe integers survive as inert exact evidence text', async (t) => {
  const original = JSON.parse(await readFile(rejected, 'utf8'))
  original.messageSeries.messageSeries.messages[0].name = '<img src=x onerror="globalThis.pwned=1"> & \u202e'
  original.messageSeries.messageSeries.messages[0].payload.synthetic_integer = 'NUMBER_PLACEHOLDER'
  const input = JSON.stringify(original).replace('"NUMBER_PLACEHOLDER"', '18446744073709551615')
  const path = `${scratch}/synthetic-hostile.json`
  await writeFile(path, input)
  const { call } = await connect(t, path)
  const shown = await call('show_operation')
  assert.equal(shown.structuredContent.evidenceJson, input)
  assert.doesNotMatch(shown.content[0].text, /<img/)
  const inspected = await call('inspect_message', { binding: shown.structuredContent.binding, messageId: original.messageSeries.messageSeries.messages[0].messageId })
  assert.match(inspected.content[0].text, /18446744073709551615/)
})

test('synthetic invalid inputs fail before protocol, bounded and with clean stdout', async () => {
  const inputs = ['{bad', ' '.repeat(1024 * 1024 + 1), '['.repeat(97) + '0' + ']'.repeat(97)]
  for (const [index, input] of inputs.entries()) {
    const path = `${scratch}/synthetic-invalid-${index}.json`
    await writeFile(path, input)
    const child = spawn(process.execPath, [entry, '--evidence', path], { cwd: scratch })
    let stdout = '', stderr = ''
    child.stdout.on('data', (data) => { stdout += data })
    child.stderr.on('data', (data) => { stderr += data })
    const [code] = await once(child, 'exit')
    assert.equal(code, 1)
    assert.equal(stdout, '')
    assert.ok(stderr.length > 0 && stderr.length < 600)
  }
  const path = `${scratch}/symlink.json`
  await symlink(accepted, path)
  const child = spawn(process.execPath, [entry, '--evidence', path], { cwd: scratch, stdio: 'ignore' })
  assert.equal((await once(child, 'exit'))[0], 1)
})

// Regressions from the independent boundary review: exercise the actual process,
// including a writerless FIFO, rather than mocking filesystem calls.
test('BOM and invalid UTF-8 fail explicitly without publishing a different byte identity', async (t) => {
  for (const [name, bytes, diagnostic] of [
    ['bom', Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), await readFile(rejected)]), /BOM/],
    ['utf8', Buffer.concat([await readFile(rejected), Buffer.from([0xff])]), /encoded data/],
  ]) {
    const path = `${scratch}/synthetic-${name}.json`
    await writeFile(path, bytes)
    await failsBeforeProtocol(t, path, diagnostic)
  }
})

async function failsBeforeProtocol(t, path, diagnostic, eof = true) {
  const child = spawn(process.execPath, [entry, '--evidence', path], { cwd: scratch })
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL') })
  let stdout = '', stderr = ''
  child.stdout.on('data', (data) => { stdout += data })
  child.stderr.on('data', (data) => { stderr += data })
  if (eof) child.stdin.end()
  const timer = setTimeout(() => child.kill('SIGKILL'), 5000)
  const exit = await once(child, 'close')
  clearTimeout(timer)
  assert.deepEqual(exit, [1, null], 'Startup fails within five seconds without forced cleanup')
  assert.equal(stdout, '')
  assert.match(stderr, diagnostic)
  assert.ok(stderr.length < 600)
  transcripts.push({ startupBoundary: path, eof, exit, stdout, stderr })
}

test('nonregular inputs fail before read; writerless FIFO is bounded with open stdin and immediate EOF', async (t) => {
  const fifo = `${scratch}/synthetic-fifo`
  execFileSync('mkfifo', [fifo])
  for (const eof of [false, true]) await failsBeforeProtocol(t, fifo, /regular file/, eof)
  for (const path of [scratch, '/dev/null']) await failsBeforeProtocol(t, path, /regular file/)
})

test('raw initialization notifications agree with advertised listChanged capabilities', async (t) => {
  const child = spawn(process.execPath, [entry, '--evidence', rejected], { cwd: scratch })
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL') })
  const messages = []
  let tail = '', stderr = ''
  child.stderr.on('data', (data) => { stderr += data })
  child.stdout.on('data', (data) => {
    tail += data
    let end
    while ((end = tail.indexOf('\n')) !== -1) {
      messages.push(JSON.parse(tail.slice(0, end)))
      tail = tail.slice(end + 1)
    }
  })
  const send = (message) => child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`)
  const response = async (id) => {
    for (let n = 0; n < 500; n++) {
      const found = messages.find((message) => message.id === id)
      if (found) return found
      await new Promise((done) => setTimeout(done, 10))
    }
    throw new Error(`Protocol response ${id} timed out: ${stderr}`)
  }
  send({ id: 1, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'boundary-regression', version: '1' } } })
  const initialized = await response(1)
  send({ method: 'notifications/initialized' })
  send({ id: 2, method: 'ping' })
  await response(2)
  const notifications = messages.filter((message) => message.method?.endsWith('/list_changed'))
  assert.ok(notifications.some((message) => message.method === 'notifications/tools/list_changed'))
  assert.ok(notifications.some((message) => message.method === 'notifications/resources/list_changed'))
  for (const notification of notifications) assert.equal(initialized.result.capabilities[notification.method.split('/')[1]].listChanged, true)
  const exit = once(child, 'close')
  child.stdin.end()
  const timer = setTimeout(() => child.kill('SIGKILL'), 2000)
  assert.deepEqual(await exit, [0, null])
  clearTimeout(timer)
  assert.equal(stderr, '')
  transcripts.push({ rawInitialization: messages })
})

test('synthetic malformed captures retain independent report text and refuse ambiguous selection', async (t) => {
  for (const variant of ['duplicate', 'mismatch']) {
    const source = JSON.parse(await readFile(rejected, 'utf8'))
    const messageId = source.messageSeries.messageSeries.messages[0].messageId
    if (variant === 'duplicate') source.messageSeries.messageSeries.messages.push(source.messageSeries.messageSeries.messages[0])
    else source.messageSeries.operationId = 'wrong-operation'
    const path = `${scratch}/synthetic-${variant}.json`
    await writeFile(path, JSON.stringify(source))
    const { call } = await connect(t, path)
    const shown = await call('show_operation')
    assert.match(shown.content[0].text, /invalid/i)
    assert.equal((await call('select_message', { binding: shown.structuredContent.binding, messageId })).isError, true)
  }
})

for (const termination of ['eof', 'SIGTERM', 'SIGINT']) test(`actual stdin lifecycle: ${termination}, JSON-only stdout`, async (t) => {
  const child = spawn(process.execPath, [entry, '--evidence', rejected], { cwd: scratch })
  t.after(() => { if (child.exitCode === null) child.kill('SIGKILL') })
  const exit = once(child, 'exit')
  let stdout = '', stderr = ''
  child.stdout.on('data', (data) => { stdout += data })
  child.stderr.on('data', (data) => { stderr += data })
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'lifecycle', version: '1' } } })}\n`)
  await Promise.race([once(child.stdout, 'data'), exit.then(() => { throw new Error(`Server exited before initialize: ${stderr}`) })])
  if (termination === 'eof') child.stdin.end()
  else child.kill(termination)
  const timeout = setTimeout(() => child.kill('SIGKILL'), 3000)
  const [code, signal] = await exit
  clearTimeout(timeout)
  assert.equal(code, 0)
  assert.equal(signal, null)
  assert.equal(stderr, '')
  for (const line of stdout.trim().split('\n')) assert.equal(JSON.parse(line).jsonrpc, '2.0')
})

test('real accepted capture integration gate', async (t) => {
  const { call } = await connect(t, accepted)
  const shown = await call('show_operation')
  assert.match(shown.content[0].text, /accepted/)
  assert.equal(shown.structuredContent.evidenceJson, await readFile(accepted, 'utf8'))
})

test('retains canonical byte hashes and writes protocol evidence', async () => {
  for (const [path, digest] of Object.entries(evidenceHashes)) assert.equal(hash(await readFile(path)), digest)
  await writeFile(`${scratch}/transcripts.json`, JSON.stringify({ node: process.version, entry, evidenceHashes, transcripts }, null, 2))
  console.error(`Protocol artifacts: ${scratch}`)
})
