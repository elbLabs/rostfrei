import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const packageRoot = fileURLToPath(new URL('../', import.meta.url))
const root = resolve(packageRoot, '../../..')
assert.ok(process.env.EVIDENCE_DIR, 'Set EVIDENCE_DIR to a directory containing both saved tutorial captures')
const evidenceDir = resolve(process.env.EVIDENCE_DIR)
const artifactRoot = resolve(process.env.ARTIFACT_DIR ?? `${root}/artifacts/studio-adapters/mcp-apps`)
await mkdir(artifactRoot, { recursive: true })
const scratch = await mkdtemp(`${artifactRoot}/qualification-`)
const env = { ...process.env, EVIDENCE_DIR: evidenceDir, PATH: `${dirname(process.execPath)}:${process.env.PATH}`, npm_config_cache: `${root}/artifacts/studio-adapters/tools/mcp-apps/npm-cache` }
const records = []
async function run(name, command, args, cwd = packageRoot, extraEnv = {}) {
  const started = Date.now()
  const child = spawn(command, args, { cwd, env: { ...env, ...extraEnv }, stdio: ['ignore', 'pipe', 'pipe'] })
  let stdout = '', stderr = ''
  child.stdout.on('data', (data) => { stdout += data })
  child.stderr.on('data', (data) => { stderr += data })
  const code = await new Promise((done, reject) => { child.on('error', reject); child.on('close', done) })
  records.push({ name, command, args, cwd, code, elapsedMs: Date.now() - started })
  await writeFile(`${scratch}/${name}.stdout.log`, stdout)
  await writeFile(`${scratch}/${name}.stderr.log`, stderr)
  console.log(`${name}: exit ${code}`)
  if (code !== 0) throw new Error(`${name} failed; see ${scratch}`)
  return stdout
}
try {
  await run('frozen-install', 'npm', ['ci', '--ignore-scripts', '--no-audit', '--no-fund'])
  await run('typecheck', 'npm', ['run', 'check'])
  await run('build', 'npm', ['run', 'build'])
  const packed = JSON.parse(await run('pack', 'npm', ['pack', '--json', '--pack-destination', scratch]))[0]
  assert.ok(packed.files.some((file) => file.path === 'dist/server.js'))
  assert.ok(packed.files.some((file) => file.path === 'dist/view.html'))
  assert.ok(packed.files.some((file) => file.path === 'dist/LICENSE'))
  assert.ok(packed.files.some((file) => file.path === 'dist/provenance.json'))
  assert.ok(!packed.files.some((file) => /(^|\/)(artifacts|node_modules|test)\//.test(file.path)))
  const tgz = `${scratch}/${packed.filename}`
  await mkdir(`${scratch}/installed`)
  await writeFile(`${scratch}/installed/package.json`, JSON.stringify({ private: true, type: 'module', dependencies: { '@rostfrei-experiments/saved-evidence-mcp-app': `file:${tgz}` } }))
  await run('installed-lock', 'npm', ['install', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'], `${scratch}/installed`)
  await run('installed-frozen', 'npm', ['ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'], `${scratch}/installed`)
  const entry = `${scratch}/installed/node_modules/@rostfrei-experiments/saved-evidence-mcp-app/dist/server.js`
  const extra = { SERVER_ENTRY: entry, ARTIFACT_DIR: scratch }
  const config = `${scratch}/cli-config.json`
  await writeFile(config, JSON.stringify({ mcpServers: { evidence: { command: process.execPath, args: [entry, '--evidence', `${evidenceDir}/reject-unavailable-bicycle.json`], protocolEra: 'legacy' } } }))
  const inspector = `${packageRoot}/node_modules/@modelcontextprotocol/inspector/clients/launcher/build/index.js`
  const inspectorEnv = { HOME: `${scratch}/cli-home`, MCP_STORAGE_DIR: `${scratch}/cli-storage`, MCP_CLIENT_CONFIG_PATH: `${scratch}/cli-client.json`, MCP_INSPECTOR_SECRET_STORE: 'memory', MCP_AUTO_OPEN_ENABLED: 'false' }
  await mkdir(inspectorEnv.HOME)
  const appInfo = await run('inspector-cli-apps', process.execPath, [inspector, '--cli', '--config', config, '--server', 'evidence', '--method', 'tools/list', '--app-info', '--advertise-apps'], scratch, inspectorEnv)
  assert.match(appInfo, /text\/html;profile=mcp-app/)
  const fallback = await run('inspector-cli-text', process.execPath, [inspector, '--cli', '--config', config, '--server', 'evidence', '--method', 'tools/call', '--tool-name', 'show_operation', '--format', 'json'], scratch, inspectorEnv)
  assert.match(fallback, /Text fallback/)
  assert.match(fallback, /BICYCLE_UNAVAILABLE/)
  await run('packed-protocol', process.execPath, [`${packageRoot}/test/protocol.test.mjs`], scratch, extra)
  await run('packed-browser', process.execPath, [`${packageRoot}/test/inspector.mjs`], scratch, extra)
  await run('browser-connection-gates', process.execPath, [`${packageRoot}/test/browser-gate.mjs`], scratch, extra)
  // Check the installed artifact against CURRENT sources after qualification,
  // including the actual core bytes embedded in its server source map.
  const hash = (bytes) => createHash('sha256').update(bytes).digest('hex')
  const provenance = JSON.parse(await readFile(`${dirname(entry)}/provenance.json`, 'utf8'))
  const sourceHashes = {}
  for (const [path, digest] of Object.entries(provenance.sourceHashes)) {
    sourceHashes[path] = hash(await readFile(resolve(packageRoot, path)))
    assert.equal(sourceHashes[path], digest, `Installed provenance agrees with current source: ${path}`)
  }
  const map = JSON.parse(await readFile(`${entry}.map`, 'utf8'))
  const embedded = {}
  for (const [index, path] of map.sources.entries()) {
    const current = await readFile(resolve(packageRoot, 'dist', path))
    embedded[path] = hash(map.sourcesContent[index])
    assert.equal(embedded[path], hash(current), `Installed source map embeds current bytes: ${path}`)
  }
  for (const name of ['evidence.ts', 'json.ts']) assert.ok(Object.keys(embedded).some((path) => path.endsWith(`/tracer-presentation/src/${name}`)))
  await writeFile(`${scratch}/source-agreement.json`, JSON.stringify({ status: 'passed', sourceHashes, embedded }, null, 2))
  await writeFile(`${scratch}/package-evidence.json`, JSON.stringify({ node: process.version, archive: tgz, sha256: createHash('sha256').update(await readFile(tgz)).digest('hex'), packed, entry }, null, 2))
} catch (error) { console.error(String(error)); process.exitCode = 1 }
finally {
  await writeFile(`${scratch}/steps.json`, JSON.stringify(records, null, 2))
  console.log(`Qualification artifacts: ${scratch}`)
}
