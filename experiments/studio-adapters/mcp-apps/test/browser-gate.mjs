import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// Actual Inspector regressions for both canonical connection failures. The
// accepted corruption repeats the independent review probe; rejected corruption
// keeps JSON parseable so it reaches the explicit server connection gate.
const root = fileURLToPath(new URL('../../../../', import.meta.url))
assert.ok(process.env.EVIDENCE_DIR, 'Set EVIDENCE_DIR to a directory containing both saved tutorial captures')
const evidenceDir = resolve(process.env.EVIDENCE_DIR)
const artifactRoot = resolve(process.env.ARTIFACT_DIR ?? `${root}/artifacts/studio-adapters/mcp-apps`)
await mkdir(artifactRoot, { recursive: true })
const scratch = await mkdtemp(`${artifactRoot}/browser-gate-`)
const names = ['rent-available-bicycle', 'reject-unavailable-bicycle']
const records = []
for (const failing of names) {
  const dir = `${scratch}/SYNTHETIC-${failing}-connection-failure`
  await mkdir(dir)
  for (const name of names) {
    let bytes = await readFile(`${evidenceDir}/${name}.json`)
    if (name === failing) {
      if (name === 'rent-available-bicycle') bytes = '{ SYNTHETIC invalid accepted evidence }'
      else {
        const source = JSON.parse(bytes)
        source.report.operation.mode = 'SYNTHETIC-invalid-mode'
        bytes = JSON.stringify(source)
      }
    }
    await writeFile(`${dir}/${name}.json`, bytes)
  }
  const child = spawn(process.execPath, [fileURLToPath(new URL('./inspector.mjs', import.meta.url))], { env: { ...process.env, EVIDENCE_DIR: dir, ARTIFACT_DIR: dir }, stdio: ['ignore', 'pipe', 'pipe'] })
  let stdout = '', stderr = ''
  child.stdout.on('data', (data) => { stdout += data })
  child.stderr.on('data', (data) => { stderr += data })
  const code = await new Promise((done, reject) => { child.on('error', reject); child.on('close', done) })
  await writeFile(`${dir}/stdout.log`, stdout)
  await writeFile(`${dir}/stderr.log`, stderr)
  assert.equal(code, 1, `Explicit ${failing} failure must fail browser qualification`)
  const artifacts = stdout.match(/Inspector artifacts: (.+)/)?.[1]
  assert.ok(artifacts, 'Failure diagnostics were retained')
  const diagnostics = JSON.parse(await readFile(`${artifacts}/diagnostics.json`, 'utf8'))
  assert.equal(diagnostics.status, 'failed')
  assert.ok(diagnostics.cases.some((item) => item.name === failing && item.status === 'blocked'), 'Failed at the actual canonical connection gate')
  assert.ok(!diagnostics.completedCaptures.includes(failing))
  records.push({ failing, code, artifacts, status: diagnostics.status })
}
await writeFile(`${scratch}/results.json`, JSON.stringify(records, null, 2))
console.log(`Both canonical connection failure regressions passed: ${scratch}`)
