// Compile the actual desktop test binary, then let the workerd harness exercise
// the native controller over pipes. The app does not gain a test control endpoint.
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { createInterface } from 'node:readline'

const root = fileURLToPath(new URL('../../', import.meta.url))
let binary = process.env.NEXUS_REMOTE_TEST_BINARY
if (!binary) {
  const cargo = spawn('cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', '--features', 'radio', '--no-run', '--message-format=json'], {
    cwd: root, stdio: ['ignore', 'pipe', 'inherit'], env: process.env,
  })
  const lines = createInterface({ input: cargo.stdout })
  lines.on('line', line => {
    try {
      const value = JSON.parse(line)
      if (value.reason === 'compiler-artifact' && value.target.name === 'tempo_lib' && value.executable) binary = value.executable
      if (value.reason === 'compiler-message' && value.message.level === 'error') process.stderr.write(value.message.rendered)
    } catch { /* cargo's non-JSON progress is not a test result */ }
  })
  const code = await new Promise(resolve => cargo.on('exit', resolve))
  if (code !== 0 || !binary) process.exit(code || 1)
}
const test = spawn(process.execPath, ['--test', 'test/native.test.mjs'], {
  cwd: fileURLToPath(new URL('../', import.meta.url)), stdio: 'inherit',
  env: { ...process.env, NEXUS_REMOTE_TEST_BINARY: binary },
})
const result = await new Promise(resolve => test.on('exit', resolve))
process.exit(result === 0 ? 0 : result || 1)
