import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdir, mkdtemp, readFile, readdir, stat, writeFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'
import { savedEvidence } from '../../../packages/tracer-presentation/test/fixtures.mjs'
import { runProcess } from './process.mjs'

process.umask(0o077)
const artifacts = resolve(process.env.ADAPTER_TEST_ARTIFACTS)
const claude = resolve(process.env.CLAUDE)
assert.equal((await stat(artifacts)).mode & 0o077, 0, 'test artifact root must be owner-private')
const runner = fileURLToPath(new URL('./run-mod.mjs', import.meta.url))
const names = ['rent-available-bicycle.json', 'reject-unavailable-bicycle.json']
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex')
const json = async (path) => JSON.parse(await readFile(path, 'utf8'))
const fixture = savedEvidence()
fixture.adapterFixture = 'SYNTHETIC adapter regression — no execution'
fixture.messageSeries.messageSeries.messages[0].payload = { marker: 'SYNTHETIC PRIVATE PAYLOAD sentinel' }
const valid = Buffer.from(JSON.stringify(fixture))

async function runInputs(label, bytes) {
  const root = await mkdtemp(join(artifacts, label + '-'))
  const inputs = join(root, 'inputs')
  await mkdir(inputs, { mode: 0o700 })
  for (const [index, data] of bytes.entries()) await writeFile(join(inputs, names[index]), data, { mode: 0o600 })
  const out = join(root, 'output')
  // A normal 022 caller umask must not leak into any runner-generated file.
  const command = [process.execPath, '--input-type=module', '-e',
    'process.umask(0o022); await import(process.argv[1])', runner,
    '--claude', claude, '--out', out, '--evidence-dir', inputs]
  const processResult = await runProcess(command, {
    cwd: root, env: { PATH: '/usr/bin:/bin', HOME: root }, output: join(root, 'runner'), timeout: 30000,
  })
  assert.equal(processResult.timedOut, false)
  assert.equal(processResult.groupCleaned, true)
  assert.equal(processResult.childrenCleaned, true)
  for (const [index, data] of bytes.entries()) {
    assert.deepEqual(await readFile(join(inputs, names[index])), data)
    assert.equal((await stat(join(inputs, names[index]))).mode & 0o777, 0o600)
  }
  const result = await json(join(out, 'result.json'))
  assert.equal(result.temporaryStateRemoved, true)
  assert.equal(result.originalEvidenceUnchanged, true)
  assert.equal(result.nativeTestsPassed, true, JSON.stringify(result))
  assert.ok(result.checks.every((check) => check.groupCleaned && check.childrenCleaned))
  return { root, out, inputs, processResult, result }
}

test('all explicitly requested invalid captures fail overall even when native tests pass', async () => {
  const invalidCapture = structuredClone(fixture)
  invalidCapture.messageSeries.messageSeries.messages[0].kind = 'invented-message-kind'
  const bytes = [Buffer.from('{invalid'), Buffer.from(JSON.stringify(invalidCapture))]
  const { out, processResult, result } = await runInputs('invalid-inputs', bytes)
  assert.equal(processResult.exit, 1)
  assert.equal(result.success, false)
  assert.equal(result.allInputsImported, false)
  assert.equal(result.importFailures.length, 2)
  assert.ok(result.importFailures[1].diagnostics.some((item) => item.code === 'INVALID_CAPTURE'))
  for (const [index, failure] of result.importFailures.entries()) assert.equal(failure.sha256, hash(bytes[index]))
  assert.match(await readFile(join(out, 'native-tests.stdout.txt'), 'utf8'), /3 pass\s+0 fail/)
})

test('one malformed UTF-8 capture fails overall and retains its actual byte digest', async () => {
  const bad = Buffer.concat([Buffer.from('{"bad":"'), Buffer.from([0xff]), Buffer.from('"}')])
  const { processResult, result } = await runInputs('invalid-utf8', [valid, bad])
  assert.equal(processResult.exit, 1)
  assert.equal(result.success, false)
  assert.equal(result.importFailures.length, 1)
  assert.equal(result.importFailures[0].sha256, hash(bad))
  assert.match(result.importFailures[0].error, /utf-8/i)
})

test('byte-identical owner-only inputs pass native selections and produce exclusively private artifacts under umask 022', async () => {
  const { out, inputs, processResult, result } = await runInputs('private-duplicate', [valid, valid])
  assert.equal(processResult.exit, 0)
  assert.equal(result.success, true)
  const staged = await json(join(out, 'inputs.json'))
  const captures = staged.captures.filter((capture) => !capture.synthetic)
  assert.equal(captures.length, 2)
  assert.equal(captures[0].captureId, captures[1].captureId)
  assert.notEqual(captures[0].source, captures[1].source)
  const events = (await readFile(join(out, 'native-tests.stderr.txt'), 'utf8')).trim().split('\n').map(JSON.parse)
  for (const surface of ['terminal', 'desktop']) {
    for (const name of names) assert.ok(events.some((event) => event.surface === surface && event.returned?.source === join(inputs, name)))
  }
  async function privateTree(path) {
    const info = await stat(path)
    assert.equal(info.mode & 0o077, 0, path)
    if (info.isDirectory()) for (const name of await readdir(path)) await privateTree(join(path, name))
  }
  await privateTree(out)
  assert.equal((await stat(out)).mode & 0o777, 0o700)
  const snapshot = join(out, 'plugin/snapshots.js')
  assert.equal((await stat(snapshot)).mode & 0o777, 0o400)
  assert.match(await readFile(snapshot, 'utf8'), /SYNTHETIC PRIVATE PAYLOAD sentinel/)
})

for (const earlyExit of [false, true]) {
  test(earlyExit ? 'cleanup also kills and reaps descendants after the leader exits' : 'hard deadline escalates and reaps a SIGTERM-ignoring leader and descendant', async () => {
    const root = await mkdtemp(join(artifacts, 'supervisor-'))
    const code = [
      'import json, os, signal, time',
      'signal.signal(signal.SIGTERM, signal.SIG_IGN)',
      'child = os.fork()',
      'print(json.dumps({"pid": os.getpid(), "group": os.getpgrp()}), flush=True)',
      ...(earlyExit ? ['if child: os._exit(0)'] : []),
      'while True: time.sleep(1)',
    ].join('\n')
    const result = await runProcess(['/usr/bin/python3', '-c', code], {
      cwd: root, env: { PATH: '/usr/bin:/bin', HOME: root }, output: join(root, 'child'), timeout: 300, grace: 50,
    })
    const pids = result.stdout.trim().split('\n').map(JSON.parse)
    try {
      assert.equal(result.timedOut, !earlyExit)
      assert.equal(result.escalated, true)
      assert.equal(result.exit, earlyExit ? 0 : -9)
      assert.equal(result.groupCleaned, true)
      assert.equal(result.childrenCleaned, true)
      assert.ok(result.durationMs < 3000, JSON.stringify(result))
      assert.equal(pids.length, 2)
      for (const { pid } of pids) assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' })
    } finally {
      // Test-owned fallback only; never address unrelated processes.
      for (const group of new Set(pids.map(({ group }) => group))) {
        try { process.kill(-group, 'SIGKILL') } catch (error) { if (error.code !== 'ESRCH') throw error }
      }
    }
  })
}

test('reviewer setsid reproduction and incomplete-owned-reaping failure', async () => {
  const root = await mkdtemp(join(artifacts, 'adopted-children-'))
  const result = await runProcess(['/usr/bin/python3',
    fileURLToPath(new URL('./supervise.test.py', import.meta.url)), process.execPath], {
    cwd: root, env: { PATH: '/usr/bin:/bin', HOME: root }, output: join(root, 'regression'), timeout: 10000,
  })
  assert.equal(result.supervisorExit, 0, result.stderr)
  assert.equal(result.childrenCleaned, true)
  assert.match(result.stderr, /Ran 2 tests/)
})
