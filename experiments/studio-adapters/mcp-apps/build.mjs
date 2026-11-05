import { build } from 'esbuild'
import { cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(fileURLToPath(import.meta.url))
process.chdir(root)
await rm('dist', { recursive: true, force: true })
await mkdir('dist', { recursive: true })
await build({ entryPoints: ['src/server.ts'], bundle: true, platform: 'node', format: 'esm', target: 'node24', packages: 'external', outfile: 'dist/server.js', sourcemap: true })
const browser = await build({ entryPoints: ['src/view.ts'], bundle: true, platform: 'browser', format: 'esm', target: 'chrome146', write: false, minify: true, legalComments: 'inline', metafile: true })
const script = browser.outputFiles[0].text.replaceAll('</script', '<\\/script')
const html = (await readFile('src/view.html', 'utf8')).replace('<script>/*__APP__*/</script>', () => `<script type="module">${script}</script>`)
await writeFile('dist/view.html', html)
await cp('../../../LICENSE', 'dist/LICENSE')
// Bundled browser dependencies retain their full license texts alongside the artifact.
const packages = new Set(Object.keys(browser.metafile.inputs).flatMap((path) => {
  const match = path.match(/^(.*node_modules\/(?:@[^/]+\/)?[^/]+)\//)
  return match ? [match[1]] : []
}))
await mkdir('dist/licenses', { recursive: true })
for (const pkg of packages) {
  const manifest = JSON.parse(await readFile(resolve(pkg, 'package.json'), 'utf8'))
  let license
  for (const name of ['LICENSE', 'LICENSE.md', 'LICENSE.txt']) {
    try { license = await readFile(resolve(pkg, name), 'utf8'); break } catch {}
  }
  if (!license) throw new Error(`Missing bundled dependency license: ${manifest.name}`)
  await writeFile(`dist/licenses/${manifest.name.replaceAll('/', '_')}.txt`, `${manifest.name}@${manifest.version}\n${license}`)
}
const hashes = {}
const coreFiles = (await readdir('../../../packages/tracer-presentation/src', { recursive: true })).filter((name) => /\.(ts|json)$/.test(name))
for (const path of ['package.json', 'package-lock.json', 'README.md', 'build.mjs', 'src/server.ts', 'src/view.ts', 'src/view.html', 'test/protocol.test.mjs', 'test/inspector.mjs', 'test/browser-gate.mjs', 'test/qualify.mjs', ...coreFiles.map((name) => `../../../packages/tracer-presentation/src/${name}`)]) hashes[path] = createHash('sha256').update(await readFile(path)).digest('hex')
await writeFile('dist/provenance.json', JSON.stringify({ node: process.version, sourceHashes: hashes, browserInputs: Object.keys(browser.metafile.inputs) }, null, 2))
