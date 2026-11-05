// Qualifies the tarball, not a source/workspace import. Acquisition/build I/O is
// test tooling only; the core performs no filesystem or network operations.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtemp, readFile, writeFile, mkdir } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import { tmpdir } from 'node:os'

const packageDirectory = fileURLToPath(new URL('../', import.meta.url))
const scratch = await mkdtemp('/tmp/opencode/tracer-presentation-package-').catch((error) => {
  if (!['EACCES', 'ENOENT'].includes(error.code)) throw error
  return mkdtemp(join(tmpdir(), 'tracer-presentation-package-'))
})
const env = { ...process.env }
const run = (command, args, cwd) => execFileSync(command, args, { cwd, env, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
const manager = process.env.npm_execpath
assert.ok(manager, 'Run this probe through pnpm --dir packages/tracer-presentation test:package')
run(process.execPath, [manager, 'pack', '--pack-destination', scratch], packageDirectory)
const tarball = join(scratch, 'rostfrei-tracer-presentation-0.0.0.tgz')
const consumer = join(scratch, 'consumer')
await mkdir(consumer)
await writeFile(join(consumer, 'package.json'), JSON.stringify({ name: 'presentation-consumer-probe', private: true, type: 'module' }))
// Install the actual packed dependency, including its declared validator, into
// an unrelated consumer tree. No workspace/root manifest or lockfile is changed.
run(process.execPath, [manager, 'add', tarball, '--ignore-scripts'], consumer)
const source = await readFile(new URL('./canonical/accepted.json', import.meta.url), 'utf8')
await writeFile(join(consumer, 'accepted.json'), source)
await writeFile(join(consumer, 'probe.mjs'), `
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { readEvidence, createSelection, resolveSelection, renderText } from '@rostfrei/tracer-presentation'
const source = readFileSync(new URL('./accepted.json', import.meta.url), 'utf8')
const view = readEvidence(source)
assert.equal(view.captureStatus, 'valid')
const selection = createSelection(view, view.series.messageSeries.messages[0].messageId)
assert.equal(resolveSelection(view, selection).outcome.outcome.status, 'accepted')
assert.equal(view.sourceText, source)
assert.match(renderText(view), /business decision: accepted/)
console.log(JSON.stringify({ node: process.version, import: import.meta.resolve('@rostfrei/tracer-presentation'), result: 'passed' }))
`)
const output = run(process.execPath, ['probe.mjs'], consumer)
assert.match(output, /node_modules.*dist\/index\.js/)
await writeFile(join(consumer, 'types.mts'), `
import { readEvidence, createSelection, resolveSelection, renderText, type Selection, type Diagnostic } from '@rostfrei/tracer-presentation'
const view = readEvidence('{}')
if (view.kind === 'operation' && view.series) {
  const selection: Selection = createSelection(view, view.series.messageSeries.messages[0].messageId)
  const diagnostics: readonly Diagnostic[] = resolveSelection(view, selection).diagnostics
  console.log(diagnostics, renderText(view, { selection }))
}
`)
const compiler = fileURLToPath(new URL('../node_modules/typescript/bin/tsc', import.meta.url))
run(process.execPath, [compiler, '--noEmit', '--strict', '--target', 'es2023', '--module', 'nodenext', '--moduleResolution', 'nodenext', 'types.mts'], consumer)
console.log(output.trim())
console.log(`Installed declarations passed strict consumer typechecking. Artifact: ${scratch}`)
