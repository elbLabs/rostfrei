// Real HTTP/JetStream qualification. Launched only by check_bike_rental_host.py
// inside the disposable NATS runner; there are no request mocks or redirects.
import assert from "node:assert/strict"
import { startStudioServer, launchStudioBrowser, closeStudioBrowser, observePage } from "../studio/scripts/browser.mjs"

const { server, url } = await startStudioServer({
  define: { "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api") },
})
let browser
try {
  browser = await launchStudioBrowser()
  const page = await browser.newPage()
  const diagnostics = observePage(page)
  await page.setViewport({ width: 1440, height: 900 })
  await page.goto(url, { waitUntil: "networkidle0" })
  await page.waitForSelector('.test-row', { timeout: 15000 })
  const names = await page.$$eval('.test-row', rows => rows.map(row => row.textContent.trim()))
  assert.equal(names.length, 5)
  for (const name of names) {
    await page.evaluate(name => {
      const row = [...document.querySelectorAll('.test-row')].find(row => row.textContent.trim() === name)
      row.click()
    }, name)
    await page.waitForFunction(name => document.querySelector('.execution-header h1')?.textContent.trim() === name, {}, name)
    await page.waitForSelector('.run-button:not(:disabled)')
    const response = page.waitForResponse(response => response.request().method() === 'POST' && new URL(response.url()).pathname.endsWith('/runs'), { timeout: 45000 })
    await page.click('.run-button')
    const report = await (await response).json()
    assert.equal(report.status, 'passed', JSON.stringify(report))
    assert.equal(report.comparison.status, 'passed')
    await page.waitForSelector('.execution-result[data-status="passed"]', { timeout: 15000 })
    assert.ok(await page.$('[data-graph-node]'))
    console.log(`Studio passed: ${name}`)
  }
  assert.deepEqual(diagnostics.pageErrors, [])
  // Chrome may request an unadvertised optional icon. Keep every application/API
  // failure fatal while allowing that browser-generated request on the live host.
  assert.deepEqual(diagnostics.httpErrors.filter(error =>
    !(error.method === "GET" && error.path === "/favicon.ico" && error.status === 404)
  ), [])
} finally {
  await closeStudioBrowser(browser)
  await server.close()
}
