// @vitest-environment jsdom
//
// A window that follows another Nexus window's cluster logins says so on its Cluster and Phone
// pills, connected or not. It holds no session of its own, so the pill must never read like its
// own connection, and the tooltip names whose cluster list and SSID apply.
import { describe, it, expect, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { NowBar } from './NowBar'
import type { AppSnapshot, FeedHealth, FeedStatus } from '../types'

const on = (state: FeedStatus['state']): FeedStatus => ({ enabled: true, lastEventSecs: null, state })

function feeds(via: boolean | undefined, state: FeedStatus['state']): FeedHealth {
  return {
    cluster: on(state),
    pskr: { enabled: false, lastEventSecs: null, state: 'off' },
    phoneCluster: on(state),
    phoneClusterHost: state === 'connected' ? 'dx.example.net' : null,
    phoneSpotsSeen: 0,
    clusterViaOtherWindow: via,
  }
}

function pills(feedHealth: FeedHealth) {
  const r = render(
    <NowBar
      snap={{ radio: { band: '20m' } } as unknown as AppSnapshot}
      prop={null}
      feedHealth={feedHealth}
      connectEnabled
      dxpedEnabled
      onNavigate={() => {}}
    />,
  )
  return [...r.container.querySelectorAll<HTMLElement>('.nb-feed')].map((el) => ({
    text: el.textContent ?? '',
    title: el.title,
  }))
}

afterEach(cleanup)

describe('the cluster pills of a window that follows another window', () => {
  it('say "other window" on both pills, connected or not', () => {
    for (const state of ['connected', 'connecting'] as const) {
      const [cluster, phone] = pills(feeds(true, state))
      for (const p of [cluster, phone]) {
        expect(p.text).toContain('other window')
        expect(p.title).toContain('Through the other Nexus window')
        expect(p.title).toContain('its cluster list and SSID apply here too')
      }
      expect(cluster.text).toContain(state === 'connected' ? 'connected' : 'connecting')
    }
  })

  it('CONTROL: a window with its own logins reads as before', () => {
    for (const via of [false, undefined]) {
      const [cluster, phone] = pills(feeds(via, 'connected'))
      expect(cluster.text).toBe('Clusterconnected')
      expect(phone.title).toContain('(dx.example.net)')
      for (const p of [cluster, phone]) {
        expect(p.text).not.toContain('other window')
        expect(p.title).not.toContain('other Nexus window')
      }
    }
  })
})
