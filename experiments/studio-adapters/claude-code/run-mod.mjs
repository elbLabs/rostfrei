#!/usr/bin/env node
import { createHash } from 'node:crypto'
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { readEvidence, createSelection, renderText, resolveSelection } from '../../../packages/tracer-presentation/src/index.ts'
import { savedEvidence } from '../../../packages/tracer-presentation/test/fixtures.mjs'
import { runProcess } from './process.mjs'

process.umask(0o077)
const { values } = parseArgs({ args: process.argv.slice(2), options: {
  claude: { type: 'string' }, out: { type: 'string' }, 'evidence-dir': { type: 'string' },
} })
if (!values.claude || !values.out) throw new Error('Usage: run-mod.mjs --claude <binary> --out <new-artifact-directory> [--evidence-dir <saved-pair>]')
const binary = resolve(values.claude)
const out = resolve(values.out)
await mkdir(out, { mode: 0o700 }) // Never reuse or change permissions on caller-owned data.
const source = dirname(fileURLToPath(import.meta.url))
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex')
const originals = []
const snapshots = []
const importFailures = []
const checks = []
const errors = []
let scratch
let temporaryStateRemoved = false
let nativeTestsPassed = false
let coreHashes = {}
const requestedSources = values['evidence-dir']
  ? ['rent-available-bicycle', 'reject-unavailable-bicycle'].map((name) => resolve(values['evidence-dir'], name + '.json'))
  : []

const save = (name, value) => writeFile(join(out, name), JSON.stringify(value, null, 2) + '\n', { mode: 0o600 })

async function copyPrivate(from, to) {
  await mkdir(to, { mode: 0o700 })
  for (const entry of await readdir(from, { withFileTypes: true })) {
    if (entry.isDirectory()) await copyPrivate(join(from, entry.name), join(to, entry.name))
    else if (entry.isFile()) await writeFile(join(to, entry.name), await readFile(join(from, entry.name)), { mode: 0o600, flag: 'wx' })
    else throw new Error('Unexpected plugin entry: ' + entry.name)
  }
}

async function fingerprint(path, prefix = '') {
  for (const entry of await readdir(path, { withFileTypes: true })) {
    const name = prefix + entry.name
    if (entry.isDirectory()) await fingerprint(join(path, entry.name), name + '/')
    else if (entry.isFile()) coreHashes[name] = hash(await readFile(join(path, entry.name)))
  }
}

async function importCapture(path, synthetic, label) {
  let sha256
  let view
  try {
    const bytes = await readFile(path)
    sha256 = hash(bytes)
    originals.push({ path, sha256 })
    const raw = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes)
    view = readEvidence(raw)
    if (view.kind !== 'operation' || !view.series?.messageSeries.messages.length) {
      throw new Error('This minimal pane requires a recorded operation and a nonempty finite capture')
    }
    snapshots.push({ source: path, synthetic, label, captureId: 'sha256:' + sha256,
      snapshotId: view.snapshotId, operationId: view.operation.operationId, summary: renderText(view),
      messages: view.series.messageSeries.messages.map((message) => {
        const selection = createSelection(view, message.messageId)
        const resolved = resolveSelection(view, selection)
        return { messageId: resolved.message.messageId, selection,
          details: renderText(view, { selection, includePayloads: true }) }
      }),
    })
  } catch (error) {
    importFailures.push({ source: path, sha256, synthetic, error: error.message,
      code: error.code, path: error.path, diagnostics: view?.diagnostics })
    console.error('Import rejected ' + path + ': ' + error.message)
  }
}

try {
  const core = resolve(source, '../../../packages/tracer-presentation/src')
  await fingerprint(core)
  // The third source is a byte-identical copy of the first, with the same IDs.
  const synthetic = JSON.stringify({ adapterFixture: 'SYNTHETIC — no execution', ...savedEvidence() })
  for (const [index, raw] of [synthetic, '\n' + synthetic, synthetic].entries()) {
    const path = join(out, 'synthetic-selection-' + index + '.json')
    await writeFile(path, raw, { mode: 0o400, flag: 'wx' })
    await importCapture(path, true, 'SYNTHETIC selection fixture ' + (index + 1))
  }
  for (const path of requestedSources) await importCapture(path, false, 'Saved capture: ' + path.split('/').pop())
  // Domain interpretation stays in Node; mods import only plugin-local files.
  const plugin = join(out, 'plugin')
  await copyPrivate(join(source, 'native-mod'), plugin)
  const snapshotPath = join(plugin, 'snapshots.js')
  await writeFile(snapshotPath, 'export const snapshots = ' + JSON.stringify(snapshots) + '\n', { mode: 0o600, flag: 'wx' })
  await chmod(snapshotPath, 0o400)
  originals.push({ path: snapshotPath, sha256: hash(await readFile(snapshotPath)) })
  scratch = await mkdtemp(join(out, 'scratch-'))
  const env = { PATH: '/usr/bin:/bin', HOME: scratch, TMPDIR: scratch,
    CLAUDE_CONFIG_DIR: join(scratch, '.claude'), XDG_CONFIG_HOME: join(scratch, 'config'),
    XDG_CACHE_HOME: join(scratch, 'cache'), XDG_DATA_HOME: join(scratch, 'data'),
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1', CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL: '1',
    DISABLE_UPDATES: '1' }
  for (const [name, args] of [
    ['version', ['--version']], ['test-help', ['plugin', 'test', '--help']],
    ['validate', ['plugin', 'validate', plugin, '--strict', '--json']],
    ['native-tests', ['plugin', 'test', plugin]],
  ]) {
    // --bare disables mods; the native runner rejects options before `plugin`.
    const command = name === 'native-tests' ? args : ['--setting-sources', '', ...args]
    const { stdout, stderr, ...result } = await runProcess([binary, ...command], {
      cwd: scratch, env, output: join(out, name),
    })
    checks.push({ name, command, ...result })
    console.log(name + ': exit ' + result.exit)
    if (result.supervisorExit !== 0) throw new Error(name + ' failed; see artifact output')
    if (name === 'validate' && JSON.parse(stdout).success !== true) throw new Error('Native mod validation did not succeed')
    if (name === 'native-tests') {
      nativeTestsPassed = /\b3 pass\s+0 fail\s+Ran 3 tests/.test(stdout)
      if (!nativeTestsPassed) throw new Error('Expected three native tests to run; see artifact output')
    }
  }
} catch (error) {
  errors.push(error.message)
} finally {
  try {
    if (scratch) await rm(scratch, { recursive: true, force: true })
    temporaryStateRemoved = true
  } catch (error) { errors.push('Scratch cleanup failed: ' + error.message) }
  let originalEvidenceUnchanged = true
  for (const original of originals) {
    try {
      if (hash(await readFile(original.path)) !== original.sha256) throw new Error('bytes changed')
    } catch (error) {
      originalEvidenceUnchanged = false
      errors.push('Input verification failed: ' + original.path + ': ' + error.message)
    }
  }
  const allInputsImported = importFailures.length === 0 && snapshots.length === 3 + requestedSources.length
  const success = nativeTestsPassed && allInputsImported && errors.length === 0
  await save('inputs.json', { originals, coreHashes, requestedSources, importFailures,
    captures: snapshots.map(({ messages, summary, ...capture }) => ({ ...capture, messageIds: messages.map((message) => message.messageId) })),
  })
  await save('result.json', { success, checks, errors, originalEvidenceUnchanged,
    temporaryStateRemoved, nativeTestsPassed, importFailures, allInputsImported,
    layer: 'native mod test runner; no signed-in UI or MCP Apps rendering',
  })
  process.exitCode = success ? 0 : 1
}
console.log('Imported captures: ' + snapshots.length + '; import failures: ' + importFailures.length)
for (const error of errors) console.error(error)
