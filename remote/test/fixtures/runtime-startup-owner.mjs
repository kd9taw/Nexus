// Child process used by browser-lifecycle.test.mjs. Its only test MUST fail while the Worker
// starts: the port it asks for is already held, so workerd's bind fails exactly as it did in
// staging run 34769726458. The parent checks that this runner then exits by itself, red, and
// leaves no workerd behind. The held socket is released either way, so any handle still keeping
// the process alive afterwards belongs to the runtime.
import { test } from 'node:test'
import { createServer } from 'node:net'
import { runtime } from '../runtime.mjs'

const holder = createServer()
await new Promise(resolve => holder.listen(0, '127.0.0.1', resolve))
const port = holder.address().port

test('intentional Worker bind failure', async () => {
  try { await runtime({ port }) } finally { await new Promise(resolve => holder.close(resolve)) }
})
