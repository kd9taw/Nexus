// Child process used by browser-lifecycle.test.mjs. It starts Chrome, prints the profile folder,
// and waits for the parent to kill it; it never calls stop().
import { chrome } from '../browser-runtime.mjs'

const browser = await chrome()
console.log(browser.profile)
setInterval(() => {}, 60_000)
