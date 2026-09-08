// The Connections-row state machine. One case per state, plus the regression that is the
// whole reason this module exists: a stored-but-never-exercised connector must NOT read as
// working. Before this, `stored` alone painted the dot, so a revoked credential stayed
// green for as long as the secret sat in the keychain.
import { describe, it, expect } from 'vitest'
import { connState, dotClass, stateLabel, whenLabel, whenText } from './connHealth'
import type { CredStatus } from '../types'

/** A healthy uploading connector; each case overrides only what it is about. */
function cred(over: Partial<CredStatus> = {}): CredStatus {
  return {
    id: 'clublog',
    connector: 'ClubLog',
    stored: true,
    identity: 'kd9taw@example.invalid',
    uploads: true,
    enabled: true,
    lastSuccessUnix: null,
    lastFailureUnix: null,
    lastFailureDetail: null,
    paused: false,
    ...over,
  }
}

describe('connState covers every row a connector can be in', () => {
  it('no credential stored beats everything else', () => {
    // Even with a failure on record: there is nothing configured to be broken.
    expect(connState(cred({ stored: false, lastFailureUnix: 5 }))).toBe('none')
  })

  it('a kill-switch beats a stale success', () => {
    expect(connState(cred({ paused: true, lastSuccessUnix: 100 }))).toBe('paused')
  })

  it('a failure NEWER than the last success is failing', () => {
    expect(connState(cred({ lastSuccessUnix: 100, lastFailureUnix: 200 }))).toBe('failing')
  })

  it('a failure OLDER than the last success is not — it recovered', () => {
    expect(connState(cred({ lastSuccessUnix: 200, lastFailureUnix: 100 }))).toBe('working')
  })

  it('a failure with no success ever is failing', () => {
    expect(connState(cred({ lastFailureUnix: 100 }))).toBe('failing')
  })

  it('a failure in the SAME SECOND as the success before it is failing', () => {
    // ⛔ The stamps are whole seconds (`now_unix`), so "newer" and "same second" are not the
    // same question, and a strict `>` answered the wrong one: an upload that succeeded and
    // then failed inside one second rendered GREEN and stayed green until the next failure.
    // Reachable, not theoretical — the auto-push worker loops with no spacing between legs
    // and a Cloudlog instance on the LAN answers in milliseconds.
    //
    // The tie resolves to `failing` because it is the honest half of an unknowable order:
    // a wrong `failing` is corrected by the next successful push, a wrong `working` is a
    // connector claiming to work at the moment it does not — the same lie in a new costume,
    // and #245's whole subject.
    expect(connState(cred({ lastSuccessUnix: 100, lastFailureUnix: 100 }))).toBe('failing')
    // The control: the reverse ordering must still recover, or "the tie is red" would be
    // satisfied by a comparison that simply always reads red.
    expect(connState(cred({ lastSuccessUnix: 101, lastFailureUnix: 100 }))).toBe('working')
  })

  it('deliberately switched off is not a problem', () => {
    expect(connState(cred({ enabled: false }))).toBe('off')
  })

  it('lookup-only connectors are never behind', () => {
    // RepeaterBook / the QRZ callbook push nothing, so "no uploads" is their normal state.
    expect(connState(cred({ id: 'repeaterbook', uploads: false, enabled: true }))).toBe('lookup')
  })

  // ⭐ THE REGRESSION. This exact row rendered green before the change.
  it('stored, on, and NEVER VERIFIED is amber — not working', () => {
    const never = cred({ lastSuccessUnix: null, lastFailureUnix: null })
    expect(connState(never)).not.toBe('working')
    expect(connState(never)).toBe('idle')
    expect(dotClass(connState(never))).toBe('warn')
    expect(stateLabel(connState(never))).toMatch(/not verified yet/)
  })

  it('a real success with nothing since is working — the positive control', () => {
    // Without this passing, a connState hard-coded to 'idle' would satisfy the case above.
    expect(connState(cred({ lastSuccessUnix: 100 }))).toBe('working')
    expect(dotClass('working')).toBe('on')
  })

  it('maps failing and paused to the same red dot, and the rest to a quiet one', () => {
    expect(dotClass('failing')).toBe('bad')
    expect(dotClass('paused')).toBe('bad')
    expect(dotClass('none')).toBe('off')
    expect(dotClass('off')).toBe('off')
    expect(dotClass('lookup')).toBe('off')
  })
})

describe('whenLabel stays readable across the range a connector actually spans', () => {
  const now = 1_754_600_000_000 // ms
  const at = (secsAgo: number) => Math.floor(now / 1000) - secsAgo

  it('says nothing for a missing or zero stamp', () => {
    expect(whenLabel(null, now)).toBe('')
    expect(whenLabel(0, now)).toBe('')
  })

  it('reads in minutes, then hours, then a date', () => {
    expect(whenLabel(at(30), now)).toBe('just now')
    expect(whenLabel(at(600), now)).toBe('10m ago')
    expect(whenLabel(at(3 * 3600), now)).toBe('3h ago')
    // The case ageLabel() could not do: a week out, "10080 min ago" is unreadable.
    const week = whenLabel(at(7 * 86400), now)
    expect(week).not.toMatch(/min|m ago|h ago/)
    expect(week).toBe(new Date(at(7 * 86400) * 1000).toLocaleDateString())
  })
})

describe('whenText says which thing happened', () => {
  const now = 1_754_600_000_000
  const at = (secsAgo: number) => Math.floor(now / 1000) - secsAgo

  it('carries the service reason on a failure', () => {
    const c = cred({ lastFailureUnix: at(600), lastFailureDetail: 'invalid app password' })
    expect(whenText(c, 'failing', now)).toBe('failed 10m ago — invalid app password')
  })

  it('reports the last success when working', () => {
    expect(whenText(cred({ lastSuccessUnix: at(600) }), 'working', now)).toBe('last upload 10m ago')
  })

  it('says lookup, not upload, for a connector that never uploads', () => {
    // #245 gave the QRZ callbook row real timestamps for the first time, and the row went
    // straight to "last upload 3m ago" — for a connector whose own `uploads` flag is false
    // and which has never uploaded anything. The line is the panel's whole explanation of
    // what the green dot means, so naming the wrong event undoes the fix it is reporting.
    const callbook = cred({ id: 'qrz-xml', uploads: false, lastSuccessUnix: at(180) })
    expect(whenText(callbook, 'working', now)).toBe('last lookup 3m ago')
    // The control: an uploading connector is unchanged.
    expect(whenText(cred({ lastSuccessUnix: at(180) }), 'working', now)).toBe('last upload 3m ago')
  })

  it('says nothing rather than something empty when there is no history', () => {
    expect(whenText(cred(), 'idle', now)).toBe('')
    expect(whenText(cred({ stored: false }), 'none', now)).toBe('')
  })
})
