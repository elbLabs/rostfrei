import { spawn } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'

// Python supervises a fresh process group and cleans its direct/adopted children
// on Linux, including detached sessions. Node stays asynchronous for cancellation.
export async function runProcess(command, { cwd, env, output, timeout = 60000, grace = 250 }) {
  const supervisor = spawn('/usr/bin/python3', [
    fileURLToPath(new URL('./supervise.py', import.meta.url)),
    '--timeout', String(timeout / 1000), '--grace', String(grace / 1000),
    '--output', output, '--', ...command,
  ], { cwd, env, stdio: ['ignore', 'ignore', 'pipe'] })
  const interrupt = () => supervisor.kill('SIGTERM')
  process.on('SIGINT', interrupt)
  process.on('SIGTERM', interrupt)
  let errorText = ''
  supervisor.stderr.setEncoding('utf8').on('data', (chunk) => { errorText += chunk })
  try {
    const code = await new Promise((resolve, reject) => {
      supervisor.on('error', reject)
      supervisor.on('close', resolve)
    })
    const result = JSON.parse(await readFile(output + '.process.json', 'utf8'))
    return { ...result, supervisorExit: code, supervisorError: errorText,
      stdout: await readFile(output + '.stdout.txt', 'utf8'),
      stderr: await readFile(output + '.stderr.txt', 'utf8') }
  } finally {
    process.off('SIGINT', interrupt)
    process.off('SIGTERM', interrupt)
  }
}
