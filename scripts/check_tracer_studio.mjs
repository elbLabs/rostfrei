// Real HTTP/JetStream qualification. Launched only by cargo xtask acceptance bike-rental-host
// inside the disposable NATS runner; there are no request mocks or redirects.
import assert from "node:assert/strict"
import { mkdtemp, rm } from "node:fs/promises"
import { createRequire } from "node:module"
import { join } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import { studioRoot, launchStudioBrowser, closeStudioBrowser, observePage, capturePage, settlePage } from "../studio/scripts/browser.mjs"

const require = createRequire(new URL('../studio/package.json', import.meta.url))
const { build, preview } = await import(pathToFileURL(require.resolve('vite')).href)
// The live token is compiled into this owned, temporary bundle only.
const directory = await mkdtemp(join(studioRoot, 'node_modules/.studio-live-'))
let server
let browser
let page
let diagnostics
try {
  await build({
    root: studioRoot,
    logLevel: 'error',
    define: {
      'import.meta.env.VITE_TRACER_API_URL': JSON.stringify('/api'),
      'import.meta.env.VITE_STUDIO_WORKSPACES': 'undefined',
    },
    build: { outDir: directory, emptyOutDir: true },
  })
  server = await preview({
    root: studioRoot,
    logLevel: 'error',
    build: { outDir: directory },
    preview: { host: '127.0.0.1', port: 0, open: false },
  })
  const url = `http://127.0.0.1:${server.httpServer.address().port}`
  browser = await launchStudioBrowser()
  page = await browser.newPage()
  diagnostics = observePage(page)
  await page.setViewport({ width: 1440, height: 900 })
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 120000 })
  const domain = '[data-inventory-view="domain"][data-inventory-state="ready"]'
  await page.waitForSelector('[data-workspace-tab="domain"][aria-selected="true"]')
  assert.equal(await page.$('[aria-label="Studio mode"]'), null)
  assert.deepEqual(
    await page.$$eval('[data-workspace-tab]', tabs => tabs.map(tab => tab.dataset.workspaceTab).sort()),
    ['commands', 'correlations', 'domain', 'infrastructure', 'observation', 'read-models', 'runs', 'tests'],
  )
  const fleet = `${domain} [data-domain-node="aggregate:bike-rental/rental-fleet"]`
  await page.waitForSelector(fleet, { visible: true, timeout: 120000 })
  await page.click('[data-workspace-tab="tests"]')
  await page.waitForSelector('.test-row, [data-connection="disconnected"], [data-connection="tests-unavailable"]', { timeout: 120000 })
  assert.equal(await page.$('.connection-notice'), null, await page.$eval('body', body => body.innerText))
  await page.waitForSelector('.test-row', { timeout: 15000 })
  const names = await page.$$eval('.test-row', rows => rows.map(row => row.textContent.trim()))
   assert.equal(names.length, 7)
  for (const name of names) {
    if (await page.$('[aria-controls="studio-navigation"][aria-expanded="false"]')) {
      await page.click('[aria-controls="studio-navigation"]')
      await settlePage(page)
    }
    const rows = await page.$$('.test-row')
    for (const row of rows) if (await row.evaluate(element => element.textContent.trim()) === name) await row.click()
    await page.waitForFunction(name => document.querySelector('.execution-header h1')?.textContent.trim() === name, {}, name)
     await page.waitForSelector('.run-button:not(:disabled)')
     await page.click('.test-panel-trigger')
    const response = page.waitForResponse(response => response.request().method() === 'POST' && new URL(response.url()).pathname.endsWith('/runs'), { timeout: 45000 })
    await page.click('.run-button')
    const report = await (await response).json()
    assert.equal(report.status, 'passed', JSON.stringify(report))
    assert.equal(report.comparison.status, 'passed')
     await page.waitForSelector('.execution-header .execution-result[data-status="passed"]', { timeout: 15000 })
     assert.ok(await page.$('.message-graph .message-card'))
     assert.equal(await page.$('.run-inspection'), null)
     await page.locator('button::-p-text(Run summary)').click()
     await page.waitForSelector('.run-inspection')
    assert.ok(await page.$('.run-inspection [data-evidence-message]'))
    const facts = await page.$$eval('.run-inspection .rf-evidence-facts > div', elements => Object.fromEntries(elements.map(element => [element.querySelector('dt').textContent, element.querySelector('dd').textContent])))
    assert.equal(facts.Test, 'Passed')
    assert.equal(facts.Command.toLowerCase(), report.operation.result.decision)
    assert.equal(facts.Capture, 'Unavailable', 'Run reports have no capture-window metadata')
    console.log(`Studio scenario passed: ${name} (command: ${report.operation.command})`)
  }
   await page.locator('button::-p-text(Back to graph)').click()
   const graphBefore = await page.$$eval('[data-graph-node]', nodes => nodes.map(node => node.dataset.nodeId))
   await page.click('[data-workspace-tab="commands"]')
   await page.click('[aria-controls="studio-command-panel"]')
   const commands = await page.$$eval('[aria-label="Command and schema"] option', options => options.map(option => option.value))
   assert.ok(!commands.some(value => value.includes('/rent-bicycle@')), 'direct hire must not be offered')
   assert.ok(commands.includes('bike-rental/collect-reserved-bicycle@1'))
   assert.ok(commands.includes('bike-rental/collect-rental-payment@1'))
   assert.ok(commands.includes('bike-rental/refund-eligible-rental-deposit@1'))
   await page.select('[aria-label="Command and schema"]', 'bike-rental/return-bicycle@2')
   for (const [field, value] of [['fleet_id', 'city-fleet'], ['bicycle_id', 'bike-42']]) {
     const selector = `[data-payload-field="${field}"] input`
     await page.waitForSelector(`${selector}:not(:disabled)`)
     await page.click(selector, {clickCount:3})
     await page.type(selector, value)
   }
   assert.equal(await page.$eval('[data-command-mode="test"]', button => button.getAttribute('aria-pressed')), 'true')
   await page.click('[data-command-mode="simulate-handler"]')
   const diagnostic = page.waitForResponse(response => response.request().method() === 'POST' && new URL(response.url()).pathname.endsWith('/simulate-handler'))
   await page.click('.command-submit')
   const result = await (await diagnostic).json()
   assert.equal(result.kind, 'handlerSimulation')
   assert.equal(result.committed, false)
   assert.ok(['accepted', 'rejected'].includes(result.outcome.decision))
   assert.ok(!('operationId' in result))
   await page.waitForSelector('[data-handler-simulation-result]')
   assert.match(await page.$eval('[data-handler-simulation-result]', panel => panel.textContent), /Uncommitted.*no acceptance receipt/)
   assert.equal(await page.$$eval('[data-graph-node]', nodes => nodes.length), 0, 'handler diagnostics do not create an observed command flow')
   await page.click('[data-workspace-tab="tests"]')
   assert.deepEqual(await page.$$eval('[data-graph-node]', nodes => nodes.map(node => node.dataset.nodeId)), graphBefore)
   console.log('Studio passed: live handler diagnostic remains separate from real Test graph')

   await page.click('[data-workspace-tab="infrastructure"]')
   const infrastructure = '[data-inventory-view="infrastructure"][data-inventory-state="ready"]'
   const sourceNode = `${infrastructure} [data-inventory-resource="source"]`
   await page.waitForSelector(sourceNode, { visible: true })
   await page.click(sourceNode)
   await page.waitForSelector(`${infrastructure} [data-inventory-stream]`)
   const liveStreams = await page.$$eval(`${infrastructure} [data-inventory-stream]`, nodes => nodes.map(node => node.dataset.inventoryStream))
   assert.ok(liveStreams.some(name => /__JOURNAL/.test(name)), 'The configured broker source reports its live journal stream')
   console.log('Studio passed: compiled domain and live JetStream inventory on the linked host')
   assert.deepEqual(diagnostics.pageErrors, [])
  // Chrome may request an unadvertised optional icon. Keep every application/API
  // failure fatal while allowing that browser-generated request on the live host.
  assert.deepEqual(diagnostics.httpErrors.filter(error =>
    !(error.method === "GET" && error.path === "/favicon.ico" && error.status === 404)
  ), [])
} catch (error) {
  if (page) {
    const directory = fileURLToPath(new URL('../artifacts/studio-live/failure/', import.meta.url))
    await capturePage(page, directory, { diagnostics, source: 'live', status: 'failed' }).catch(() => {})
    console.error(`Live Studio failure artifacts: ${directory}`)
  }
  throw error
} finally {
  try {
    await closeStudioBrowser(browser)
  } finally {
    if (server) {
      server.httpServer.closeAllConnections()
      await new Promise(resolve => server.httpServer.close(resolve))
    }
    await rm(directory, { recursive: true, force: true })
  }
}
