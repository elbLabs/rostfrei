#!/usr/bin/env node
import { open, writeFile } from 'node:fs/promises'
import { constants } from 'node:fs'
import { createHash } from 'node:crypto'
import { parseArgs } from 'node:util'
import { readEvidence, renderMermaid, renderText, summarizeEvidence } from '../dist/index.js'

const escape = (text) => text.replace(/[&<>"'`*_[\]\\]/g, (char) => `&#${char.codePointAt(0)};`)

async function main() {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: { format: { type: 'string', default: 'markdown' }, output: { type: 'string' } } })
  if (positionals.length !== 1 || !['markdown', 'mermaid'].includes(values.format)) throw new Error('Usage: rostfrei-evidence saved.json [--format markdown|mermaid] [--output new-file]')
  const file = await open(positionals[0], constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK)
  const maximum = 8 * 1024 * 1024
  let bytes
  try {
    const stat = await file.stat()
    if (!stat.isFile() || stat.size > maximum) throw new Error('Evidence must be a regular file of at most 8 MiB')
    const buffer = Buffer.alloc(maximum + 1)
    let size = 0
    while (size < buffer.length) {
      const result = await file.read(buffer, size, buffer.length - size, null)
      if (!result.bytesRead) break
      size += result.bytesRead
    }
    if (size > maximum) throw new Error('Evidence exceeds 8 MiB')
    bytes = buffer.subarray(0, size)
  } finally { await file.close() }
  const source = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes)
  if (source.startsWith('\uFEFF')) throw new Error('UTF-8 BOM is unsupported; evidence bytes were not changed')
  const view = readEvidence(source)
  let graph, unavailable
  try { graph = renderMermaid(view) } catch (error) {
    if (!['UNSUPPORTED_EVIDENCE', 'CAPTURE_UNAVAILABLE'].includes(error.code)) throw error
    unavailable = error.message
  }
  if (values.format === 'mermaid' && !graph) throw new Error(unavailable)
  const summary = summarizeEvidence(view)
  const sha256 = createHash('sha256').update(bytes).digest('hex')
  const markdown = [
    `# ${escape(summary.title)}`, '',
    ...summary.facts.map((fact) => `${fact.label}: ${escape(fact.value)}  `), '',
    ...summary.issues.map((line) => `- ${escape(line)}`), '',
    ...summary.limitations.map((line) => `- ${escape(line)}`), '',
    graph ? `\`\`\`mermaid\n${graph}\n\`\`\`` : escape(unavailable), '',
    '<details>\n<summary>Evidence details and capture identity</summary>', '',
    `SHA-256: ${sha256}`, '',
    `\`\`\`text\n${renderText(view, { includePayloads: true })}\n\`\`\``, '', '</details>', '',
  ].join('\n')
  const output = values.format === 'mermaid' ? `${graph}\n%% SHA-256: ${sha256}\n` : markdown
  if (values.output) await writeFile(values.output, output, { flag: 'wx', mode: 0o600 })
  else process.stdout.write(output)
}

main().catch((error) => {
  console.error(JSON.stringify(error instanceof Error ? error.message.slice(0, 500) : 'Export failed'))
  process.exitCode = 1
})
