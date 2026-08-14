/** Vitest setup — runs before every test file.
 *
 * WHY THIS EXISTS. Node 25 ships a built-in `localStorage` global. It is only functional
 * when the process was started with a valid `--localstorage-file`; without one, Node
 * warns ("`--localstorage-file` was provided without a valid path") and leaves an object
 * that has no `clear()`. Because that global already exists, jsdom never installs its own
 * over it — measured inside a jsdom test on Node 25.6.1:
 *
 *     window === globalThis = true
 *     globalThis.localStorage = object, .clear = undefined, constructor = undefined
 *     window.localStorage === globalThis.localStorage = true
 *
 * So every suite that resets storage between cases dies in `beforeEach` with
 * "localStorage.clear is not a function": 44 test files and 537 tests on this project,
 * all green on Node 23.11.0 with no code change. The suite's result was a function of
 * the Node minor rather than of the code under test, and the message names storage
 * rather than the version, so it reads as a real regression.
 *
 * WHY NOT PIN THE TOOLCHAIN AWAY FROM NODE 25. An `engines` range with `engine-strict`
 * makes EVERY transitive dependency's range blocking too — on this dependency tree that
 * also refuses Node 23, a version that runs the suite green (`@asamuzakjp/css-color`
 * wants `^20.19.0 || ^22.12.0 || >=24.0.0`). Banning a working version to block a broken
 * one is the wrong trade.
 *
 * WHY NOT RESTORE JSDOM'S. There is nothing to restore: jsdom's implementation was never
 * installed, because the global was already occupied. The only fix is to supply one.
 *
 * Deliberately narrow: it replaces a storage global ONLY when that global cannot `clear()`.
 * On Node <= 24, jsdom's own storage is present and functional, and this is a no-op.
 */

/** A minimal `Storage` that matches the SHAPE of the real one, not merely its methods.
 *
 *  The distinction is load-bearing and was found by test, not by reading the spec: a
 *  real `Storage` keeps `getItem`/`setItem`/`clear`/… on `Storage.prototype` and exposes
 *  each STORED KEY as an own enumerable property, so `Object.keys(localStorage)` returns
 *  the keys an operator's settings live under. Building this as an object literal put the
 *  methods on the instance instead, and `Object.keys()` then answered
 *  `['clear','getItem','key','length','removeItem','setItem']` — which is exactly what
 *  `windowScope.test.ts` diffs to prove one surface cannot overwrite another's keys.
 *
 *  So: methods on the prototype (class), entries mirrored as own enumerable properties.
 *  Property WRITES (`localStorage.foo = 'x'`) are still not intercepted — that would need
 *  a Proxy, nothing in this suite does it, and a Proxy would hide real failures. */
class MemoryStorage {
  #map = new Map<string, string>()

  get length(): number {
    return this.#map.size
  }

  clear(): void {
    for (const k of this.#map.keys()) delete (this as unknown as Record<string, unknown>)[k]
    this.#map.clear()
  }

  getItem(key: string): string | null {
    const k = String(key)
    return this.#map.has(k) ? (this.#map.get(k) as string) : null
  }

  key(index: number): string | null {
    return Array.from(this.#map.keys())[index] ?? null
  }

  removeItem(key: string): void {
    const k = String(key)
    this.#map.delete(k)
    delete (this as unknown as Record<string, unknown>)[k]
  }

  setItem(key: string, value: string): void {
    const k = String(key)
    const v = String(value)
    this.#map.set(k, v)
    Object.defineProperty(this, k, {
      value: v,
      enumerable: true,
      configurable: true,
      writable: true,
    })
  }
}

function memoryStorage(): Storage {
  return new MemoryStorage() as unknown as Storage
}

for (const name of ['localStorage', 'sessionStorage'] as const) {
  const current = (globalThis as unknown as Record<string, Storage | undefined>)[name]
  if (!current || typeof current.clear !== 'function') {
    Object.defineProperty(globalThis, name, {
      value: memoryStorage(),
      configurable: true,
      writable: true,
    })
  }
}
