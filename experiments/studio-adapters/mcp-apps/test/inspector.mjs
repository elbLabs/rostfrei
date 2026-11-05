import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { once } from 'node:events'
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { createServer } from 'node:net'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import puppeteer from 'puppeteer-core'

const packageRoot = fileURLToPath(new URL('../', import.meta.url))
const root = resolve(packageRoot, '../../..')
assert.ok(process.env.EVIDENCE_DIR, 'Set EVIDENCE_DIR to a directory containing both saved tutorial captures')
const evidenceDir = resolve(process.env.EVIDENCE_DIR)
const chrome = process.env.CHROME_PATH ?? process.env.CHROME_BIN
assert.ok(chrome, 'Set CHROME_PATH or CHROME_BIN to a provisioned Chrome executable')
const executablePath = resolve(chrome)
const entry = resolve(process.env.SERVER_ENTRY ?? `${packageRoot}/dist/server.js`)
const artifactRoot = resolve(process.env.ARTIFACT_DIR ?? `${root}/artifacts/studio-adapters/mcp-apps`)
await mkdir(artifactRoot, { recursive: true })
const scratch = await mkdtemp(`${artifactRoot}/inspector-`)
const launcher = `${packageRoot}/node_modules/@modelcontextprotocol/inspector/clients/launcher/build/index.js`
const token = randomBytes(32).toString('hex')
async function freePort() {
  const listener = createServer()
  listener.listen(0, '127.0.0.1')
  await once(listener, 'listening')
  const port = listener.address().port
  await new Promise((done) => listener.close(done))
  return port
}
const port = await freePort()
const sandboxPort = await freePort()
const canonicalNames = ['reject-unavailable-bicycle', 'rent-available-bicycle']
const canonical = Object.fromEntries(await Promise.all(canonicalNames.map(async (name) => {
  const bytes = await readFile(`${evidenceDir}/${name}.json`)
  return [name, { json: bytes.toString('utf8'), sha256: createHash('sha256').update(bytes).digest('hex') }]
})))
const hostile = JSON.parse(canonical['reject-unavailable-bicycle'].json)
hostile.messageSeries.messageSeries.messages[0].name = 'SYNTHETIC <img src=x onerror="globalThis.pwned=1">'
hostile.messageSeries.messageSeries.messages[0].payload.synthetic_integer = 'NUMBER_PLACEHOLDER'
await writeFile(`${scratch}/synthetic-hostile.json`, JSON.stringify(hostile).replace('"NUMBER_PLACEHOLDER"', '18446744073709551615'))
const targets = [...canonicalNames.map((name) => [name, `${evidenceDir}/${name}.json`]), ['SYNTHETIC-hostile-label', `${scratch}/synthetic-hostile.json`]]
const config = { mcpServers: Object.fromEntries(targets.map(([name, path]) => [name, { command: process.execPath, args: [entry, '--evidence', path], protocolEra: 'legacy' }])) }
await writeFile(`${scratch}/config.json`, JSON.stringify(config, null, 2))
const hostEnv = {
  PATH: `${dirname(process.execPath)}:${process.env.PATH}`, HOME: `${scratch}/home`,
  HOST: '127.0.0.1', CLIENT_PORT: String(port), MCP_SANDBOX_PORT: String(sandboxPort), MCP_APP_ORIGIN_PORT: '0',
  MCP_STORAGE_DIR: `${scratch}/storage`,
  MCP_CLIENT_CONFIG_PATH: `${scratch}/client.json`, MCP_INSPECTOR_SECRET_STORE: 'memory',
  MCP_INSPECTOR_API_TOKEN: token, MCP_AUTO_OPEN_ENABLED: 'false',
}
await mkdir(hostEnv.HOME, { recursive: true })
const host = spawn(process.execPath, [launcher, '--web', '--config', `${scratch}/config.json`], { env: hostEnv, cwd: scratch, detached: true, stdio: ['ignore', 'pipe', 'pipe'] })
let hostLog = ''
host.stdout.on('data', (data) => { hostLog += data.toString().replaceAll(token, '[REDACTED]') })
host.stderr.on('data', (data) => { hostLog += data.toString().replaceAll(token, '[REDACTED]') })
let browser, page
const diagnostics = { node: process.version, inspector: '2.14.0', entry, port, sandboxPort, evidenceHashes: Object.fromEntries(Object.entries(canonical).map(([name, data]) => [name, data.sha256])), requests: [], console: [], errors: [], messages: [], cases: [], completedCaptures: [] }
try {
  const url = `http://127.0.0.1:${port}`
  for (let retry = 0; ; retry++) {
    try { if ((await fetch(`${url}/healthz`)).ok) break } catch {}
    if (retry > 150 || host.exitCode !== null) throw new Error(`Inspector startup failed: ${hostLog}`)
    await new Promise((done) => setTimeout(done, 100))
  }
  assert.equal((await fetch(`${url}/api/config`)).status, 401, 'Inspector API authentication remains enabled')
  // Chromium's singleton Unix socket has a 108-byte path limit. A relative TMPDIR
  // inside this owned CWD avoids a deep artifact path triggering SIGTRAP at startup.
  process.chdir(scratch)
  browser = await puppeteer.launch({
    executablePath,
    headless: true, userDataDir: `${scratch}/chrome-profile`,
    args: process.env.BROWSER_NO_SANDBOX === '1' ? ['--no-sandbox', '--disable-dev-shm-usage'] : [],
    env: { ...process.env, TMPDIR: '.', ...(process.env.BROWSER_LIBS ? { LD_LIBRARY_PATH: process.env.BROWSER_LIBS } : {}) },
  })
  diagnostics.browser = await browser.version()
  page = await browser.newPage()
  await page.setViewport({ width: 1440, height: 1100 })
  page.on('console', (message) => diagnostics.console.push({ type: message.type(), text: message.text().replaceAll(token, '[REDACTED]') }))
  page.on('pageerror', (error) => diagnostics.errors.push(String(error)))
  page.on('request', (request) => diagnostics.requests.push({ url: request.url().replaceAll(token, '[REDACTED]'), method: request.method(), body: request.postData() }))
  // Observe official ui/* exchanges; no custom bridge or renderer is installed.
  await page.exposeFunction('recordAppMessage', (data) => diagnostics.messages.push(data))
  await page.evaluateOnNewDocument(() => window.addEventListener('message', (event) => {
    if (event.data?.jsonrpc === '2.0') window.recordAppMessage(event.data)
  }))
  await page.goto(url, { waitUntil: 'domcontentloaded' })
  await page.waitForSelector('[role="switch"]')
  async function openApp() {
    await page.waitForSelector('input[value="Apps"]')
    await page.$eval('input[value="Apps"]', (input) => input.click())
    await page.waitForSelector('[data-testid="apps-form"]')
    await page.evaluate(() => [...document.querySelectorAll('button')].find((button) => button.innerText.startsWith('Show saved operation')).click())
    // Inspector auto-opens tools with an empty argument schema when the row is selected.
    await page.waitForSelector('[data-app-status="ready"]')
    const frame = page.frames().find((frame) => frame.url() === 'about:srcdoc')
    assert.ok(frame, 'Official Inspector sandbox created the app iframe')
    await frame.waitForSelector('#reader:not([hidden])')
    return frame
  }
  async function saveFrame(name, frame) {
    const state = await frame.evaluate(() => ({ text: document.body.innerText, theme: document.documentElement.dataset.theme, width: innerWidth, height: innerHeight, scrollWidth: document.documentElement.scrollWidth, selected: document.querySelector('[aria-pressed="true"]')?.getAttribute('data-message-id'), focus: document.activeElement?.id, bounds: document.querySelector('main').getBoundingClientRect().toJSON() }))
    assert.ok(state.width > 0 && state.height > 0 && state.bounds.width > 0)
    assert.ok(state.scrollWidth <= state.width, `No horizontal overflow: ${name}`)
    await writeFile(`${scratch}/${name}.json`, JSON.stringify(state, null, 2))
    await frame.evaluate(() => window.scrollTo(0, 0))
    await page.screenshot({ path: `${scratch}/${name}.png`, fullPage: true })
    return state
  }
  for (const [index, [name]] of targets.entries()) {
    await page.setViewport({ width: 1440, height: 1100 })
    if (index) {
      await page.goto(url, { waitUntil: 'domcontentloaded' })
      await page.waitForSelector('[role="switch"]')
    }
    await page.$$eval('[role="switch"]', (inputs, index) => inputs[index].click(), index)
    try {
      await page.waitForSelector('[data-testid="connection-status"][data-status="connected"]', { timeout: 10000 })
    } catch (error) {
      diagnostics.cases.push({ name, status: 'blocked', error: await page.$eval('[data-testid="connection-status"]', (e) => e.dataset.errorMessage) })
      await page.screenshot({ path: `${scratch}/${name}-connection-blocker.png`, fullPage: true })
      throw error
    }
    await page.waitForFunction(() => document.body.innerText.includes('RESOURCES/TEMPLATES/LIST'))
    let frame = await openApp()
    const expected = name.startsWith('SYNTHETIC') ? hostile : JSON.parse(canonical[name].json)
    const messageId = expected.messageSeries.messageSeries.messages[0].messageId
    const selector = `[data-message-id="${messageId}"]`
    await frame.focus(selector)
    await page.keyboard.press('Enter')
    await frame.waitForFunction(() => document.getElementById('status').textContent.includes('confirmed by this server connection'))
    assert.match(await frame.$eval('#details', (e) => e.textContent), name === 'rent-available-bicycle' ? /bike-42/ : /bike-99/)
    if (name === 'rent-available-bicycle') {
      // Rapid local choices still serialize through the genuine host bridge.
      const ids = expected.messageSeries.messageSeries.messages.map((message) => message.messageId)
      await frame.$$eval('[data-message-id]', (buttons) => buttons.forEach((button) => button.click()))
      await frame.waitForFunction(() => document.getElementById('status').textContent.includes('Selected message confirmed'))
      assert.equal(await frame.$eval('[aria-pressed="true"]', (e) => e.dataset.messageId), ids.at(-1))
      await frame.click(selector)
      await frame.waitForFunction(() => document.getElementById('status').textContent.includes('Selected message confirmed'))
    }
    const state = await saveFrame(`${name}-official-host`, frame)
    assert.equal(state.selected, messageId)
    const before = await frame.$eval('#binding', (e) => e.textContent)
    await frame.click('#display')
    assert.equal(await frame.$eval('#plain', (e) => e.hidden), false)
    await frame.click('#display')
    assert.equal(await frame.$eval('#binding', (e) => e.textContent), before)
    const sidebar = await page.$('[aria-label="Close monitoring sidebar"]')
    if (sidebar) await sidebar.click()
    await page.click('[aria-label="Maximize"]')
    for (const theme of ['light', 'dark']) {
      // Inspector keeps a wide minimum layout. At a narrow viewport, clicking its
      // right-hand theme control scrolls the app offscreen and pauses iframe rAF.
      // Exercise host controls at the supported wide size before each reflow pass.
      await page.setViewport({ width: 1440, height: 1100 })
      const current = await frame.evaluate(() => document.documentElement.dataset.theme)
      if (current !== theme) await page.click('[aria-label="Toggle color scheme"]')
      await frame.waitForFunction((theme) => document.documentElement.dataset.theme === theme, { polling: 100 }, theme)
      for (const [size, width, height] of [['wide', 1440, 1100], ['narrow', 390, 844]]) {
        await page.$eval('iframe', (iframe) => { iframe.style.width = '100%'; iframe.style.maxWidth = '' })
        await page.setViewport({ width, height })
        let measured = await saveFrame(`${name}-${theme}-native-${size}`, frame)
        if (size === 'narrow' && measured.width > width) {
          diagnostics.cases.push({ name, theme, status: 'host-narrow-layout-limitation', viewportWidth: width, actualFrameWidth: measured.width })
        }
        // Explicit synthetic sizing control, still inside the actual Inspector
        // sandbox/bridge. Inspector's own page currently has a wide minimum width.
        await page.$eval('iframe', (iframe, width) => { iframe.style.width = `${width}px`; iframe.style.maxWidth = `${width}px` }, size === 'narrow' ? 390 : 1000)
        await frame.waitForFunction((width) => innerWidth === width, {}, size === 'narrow' ? 390 : 1000)
        measured = await saveFrame(`${name}-${theme}-synthetic-container-${size}`, frame)
        assert.equal(measured.selected, messageId)
        diagnostics.cases.push({ name, theme, size, sizing: 'synthetic outer iframe width in official host', status: 'rendered-and-selection-confirmed', viewport: { width, height }, frame: { width: measured.width, height: measured.height } })
      }
    }
    if (name.startsWith('SYNTHETIC')) {
      assert.equal(await frame.evaluate(() => globalThis.pwned), undefined)
      assert.equal(await frame.$$eval('#messages img', (nodes) => nodes.length), 0)
      assert.match(await frame.$eval('#details', (e) => e.textContent), /18446744073709551615/)
    }
    // Focus restoration after a narrow keyboard selection.
    await frame.focus(selector)
    await page.keyboard.press('Enter')
    await frame.waitForFunction(() => document.getElementById('status').textContent.includes('confirmed by this server connection'))
    assert.equal(await frame.evaluate(() => document.activeElement.id), 'details-title')
    await page.screenshot({ path: `${scratch}/${name}-narrow-selected-details.png`, fullPage: true })
    await frame.evaluate(() => {
      const text = document.getElementById('details').firstChild
      const start = text.textContent.indexOf('  payload:')
      if (start < 0) throw new Error('Expected selected payload text')
      const range = document.createRange()
      range.setStart(text, start)
      range.setEnd(text, start + 10)
      window.scrollBy(0, range.getBoundingClientRect().top - 24)
    })
    await page.screenshot({ path: `${scratch}/${name}-narrow-selected-payload.png`, fullPage: true })
    await frame.click('#back')
    assert.equal(await frame.evaluate(() => document.activeElement.dataset.messageId), messageId)
    // Clear and reselect are genuine server-backed operations, not presentation mocks.
    await frame.click('#clear')
    await frame.waitForFunction(() => document.getElementById('status').textContent.startsWith('Selection cleared and confirmed'))
    await frame.click(selector)
    await frame.waitForFunction(() => document.getElementById('status').textContent.includes('Selected message confirmed'))
    await page.setViewport({ width: 1440, height: 1100 })
    await page.$eval('iframe', (iframe) => { iframe.style.width = '320px'; iframe.style.maxWidth = '320px' })
    await frame.waitForFunction(() => innerWidth === 320)
    await saveFrame(`${name}-synthetic-container-320`, frame)
    await frame.evaluate(() => { document.documentElement.style.fontSize = '32px' })
    await saveFrame(`${name}-synthetic-text-200percent`, frame)
    await frame.evaluate(() => { document.documentElement.style.fontSize = '' })
    const appTree = await frame.accessibility.snapshot({ interestingOnly: false })
    assert.match(JSON.stringify(appTree), /Captured messages/)
    assert.match(JSON.stringify(appTree), /Message details/)
    assert.match(JSON.stringify(appTree), /confirmed by this server connection/)
    await writeFile(`${scratch}/${name}-app-accessibility.json`, JSON.stringify(appTree, null, 2))
    // Close/reopen via Inspector: same connection, fresh binding, preserved selection.
    await page.click('[aria-label="Close"]')
    frame = await openApp()
    assert.equal(await frame.$eval('[aria-pressed="true"]', (e) => e.dataset.messageId), messageId)
    assert.notEqual(await frame.$eval('#binding', (e) => e.textContent), before)
    if (name === 'rent-available-bicycle') {
      // Reuse the review's real-host forwarding probes, selecting a DIFFERENT ID.
      // Width injection is explicitly synthetic. Screenshots never scroll to top.
      const eventId = expected.messageSeries.messageSeries.messages[1].messageId
      const eventSelector = `[data-message-id="${eventId}"]`
      const narrow = async () => {
        await page.$eval('iframe', (iframe) => { iframe.style.width = '390px'; iframe.style.maxWidth = '390px' })
        await frame.waitForFunction(() => innerWidth === 390)
      }
      const reopen = async (id) => {
        await page.click('[aria-label="Close"]')
        frame = await openApp()
        assert.equal(await frame.$eval('#reader', (e) => e.dataset.sharingState), 'confirmed')
        assert.equal(await frame.evaluate(() => document.querySelector('[aria-pressed="true"]')?.getAttribute('data-message-id') ?? null), id)
        await narrow()
      }
      const feedback = async (label, phase, local, confirmed) => {
        await frame.waitForFunction((phase) => document.getElementById('reader').dataset.sharingState === phase, { timeout: 15000 }, phase)
        const state = await frame.evaluate(() => ({ text: document.getElementById('detail-status').textContent, bounds: document.getElementById('detail-status').getBoundingClientRect().toJSON(), width: innerWidth, height: innerHeight, focus: document.activeElement.id, local: document.querySelector('[aria-pressed="true"]')?.getAttribute('data-message-id') ?? null, live: document.getElementById('status').getAttribute('aria-live'), liveText: document.getElementById('status').textContent }))
        assert.equal(state.local, local)
        assert.ok(state.text.includes(`Last server-confirmed selection: ${confirmed ?? 'none'}.`))
        assert.doesNotMatch(state.text, /NOT shared/)
        assert.equal(state.live, 'polite')
        assert.equal(state.liveText, state.text)
        assert.ok(state.bounds.top >= 0 && state.bounds.bottom <= state.height, `Feedback visible at natural detail scroll: ${label} ${JSON.stringify(state)}`)
        const tree = await frame.accessibility.snapshot({ interestingOnly: false })
        assert.ok(JSON.stringify(tree).includes(state.text.split('\n')[0]), 'App accessibility tree includes sharing feedback')
        await writeFile(`${scratch}/${label}.json`, JSON.stringify({ ...state, appAccessibility: tree }, null, 2))
        await page.screenshot({ path: `${scratch}/${label}.png`, fullPage: true })
        diagnostics.cases.push({ name, status: 'failure-feedback-qualified', label, phase, local, confirmed, sizing: 'SYNTHETIC 390px outer iframe' })
      }
      await narrow()
      for (const mode of ['transport-denial', 'explicit-refusal']) {
        await page.setRequestInterception(true)
        const denyWrite = (request) => {
          const body = request.postData()
          const data = body && request.url().endsWith('/api/mcp/send') ? JSON.parse(body) : null
          if (data?.message?.params?.name !== 'select_message') return void request.continue()
          if (mode === 'transport-denial') return void request.respond({ status: 403, contentType: 'application/json', body: JSON.stringify({ error: 'SYNTHETIC write forwarding denial' }) })
          data.message.params.arguments.binding.sha256 = '0'.repeat(64)
          void request.continue({ postData: JSON.stringify(data) }) // Actual server isError, no write.
        }
        page.on('request', denyWrite)
        await frame.focus(eventSelector)
        await page.keyboard.press('Enter')
        await feedback(`SYNTHETIC-${mode}-different-selection`, mode === 'transport-denial' ? 'unknown' : 'refused', eventId, messageId)
        await frame.click('#clear-details')
        await feedback(`SYNTHETIC-${mode}-clear`, mode === 'transport-denial' ? 'unknown' : 'refused', null, messageId)
        page.off('request', denyWrite)
        await page.setRequestInterception(false)
        await reopen(messageId)
      }
      // Hold only readback to measure write acknowledgment, then fail it. Reopen
      // proves the write really persisted despite the unavailable confirmation.
      for (const clear of [false, true]) {
        let held
        await page.setRequestInterception(true)
        const denyReadback = (request) => {
          const body = request.postData()
          if (request.url().endsWith('/api/mcp/send') && body && JSON.parse(body).message?.params?.name === 'get_selection') held = request
          else void request.continue()
        }
        page.on('request', denyReadback)
        if (clear) {
          await frame.focus('#details-title')
          await frame.$eval('#details-title', (e) => e.scrollIntoView({ block: 'start' }))
          await frame.click('#clear-details')
        } else {
          await frame.focus(eventSelector)
          await page.keyboard.press('Enter')
        }
        const label = `SYNTHETIC-readback-${clear ? 'clear' : 'different-selection'}`
        await feedback(`${label}-acknowledged`, 'acknowledged', clear ? null : eventId, clear ? eventId : messageId)
        assert.ok(held, 'Readback reached actual host forwarding')
        await held.respond({ status: 403, contentType: 'application/json', body: JSON.stringify({ error: 'SYNTHETIC readback denial after acknowledged write' }) })
        await feedback(`${label}-unknown`, 'unknown', clear ? null : eventId, clear ? eventId : messageId)
        page.off('request', denyReadback)
        await page.setRequestInterception(false)
        await reopen(clear ? null : eventId)
      }
    }
    // Synthetic missing-resource control uses the actual host and actual server
    // error path by substituting an unknown URI on one resources/read request.
    await page.click('[aria-label="Close"]')
    await page.setRequestInterception(true)
    const denyResource = (request) => {
      const body = request.postData()
      if (request.url().endsWith('/api/mcp/send') && body) {
        const data = JSON.parse(body)
        if (data.message?.method === 'resources/read') {
          data.message.params.uri = 'ui://rostfrei/synthetic-missing.html'
          void request.continue({ postData: JSON.stringify(data) })
          return
        }
      }
      void request.continue()
    }
    page.on('request', denyResource)
    await page.evaluate(() => [...document.querySelectorAll('button')].find((button) => button.innerText.startsWith('Show saved operation')).click())
    await page.waitForFunction(() => /resource not found|failed to (load|read)|unknown resource|resource.*error/i.test(document.body.innerText), { timeout: 15000 })
    await writeFile(`${scratch}/${name}-synthetic-resource-failure.txt`, await page.evaluate(() => document.body.innerText))
    await page.screenshot({ path: `${scratch}/${name}-synthetic-resource-failure.png`, fullPage: true })
    assert.equal(await page.$('[data-app-status="ready"]'), null)
    page.off('request', denyResource)
    await page.setRequestInterception(false)
    await page.click('[aria-label="Disconnect from server"]')
    await page.waitForSelector('[data-testid="connection-status"][data-status="disconnected"]')
    diagnostics.completedCaptures.push(name)
  }
  const session = await page.createCDPSession()
  await writeFile(`${scratch}/accessibility.json`, JSON.stringify(await session.send('Accessibility.getFullAXTree'), null, 2))
  for (const name of canonicalNames) assert.equal(createHash('sha256').update(await readFile(`${evidenceDir}/${name}.json`)).digest('hex'), canonical[name].sha256)
  assert.deepEqual(diagnostics.requests.filter((request) => !request.url.startsWith('http://127.0.0.1:') && !request.url.startsWith('blob:') && !request.url.startsWith('data:')), [], 'No external browser requests')
  for (const name of canonicalNames) assert.ok(diagnostics.completedCaptures.includes(name), `Canonical browser gate completed: ${name}`)
  diagnostics.status = 'passed'
} catch (error) {
  diagnostics.status = 'failed'
  diagnostics.failure = String(error)
  if (page) { await writeFile(`${scratch}/failure-dom.html`, await page.content()); await page.screenshot({ path: `${scratch}/failure.png`, fullPage: true }) }
  process.exitCode = 1
} finally {
  await browser?.close()
  try { process.kill(-host.pid, 'SIGTERM') } catch {}
  if (host.exitCode === null) await Promise.race([once(host, 'exit'), new Promise((done) => setTimeout(done, 5000))])
  if (host.exitCode === null) { try { process.kill(-host.pid, 'SIGKILL') } catch {} }
  await writeFile(`${scratch}/host.log`, hostLog)
  await writeFile(`${scratch}/diagnostics.json`, JSON.stringify(diagnostics, null, 2))
  console.log(`Inspector artifacts: ${scratch}`)
  if (diagnostics.failure) console.error(diagnostics.failure)
}
