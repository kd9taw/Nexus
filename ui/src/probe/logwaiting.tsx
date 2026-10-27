// THE OPERATE STRIP'S LOG BUTTON AND ITS WAITING COUNT IN A REAL BROWSER, for `scripts/browser-probe --desktop
// logwaiting` (ui/probe/logwaiting-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the
// count of contacts waiting for Log QSO moves or wraps Log or anything beside it, whether the strip clips it, or
// whether it covers a control. This is the real OperateQsoStrip on the app's stylesheets, mid-contact, with every
// control it carries in the cockpit. The handlers record what they were asked (`window.__calls`); nothing is sent.
//
// `?waiting=N` contacts waiting for Log QSO (0 = none); `&dx=0` no station on screen (after Call CQ);
// `&theme=light|dark`; `&control=1` is the CONTROL: the count put back in the flow, wide, which the probe must
// catch moving the controls.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { useViewport } from '../useViewport'
import { OperateQsoStrip } from '../components/OperateQsoStrip'
import type { QsoStatus, RadioStatus } from '../types'

const params = new URLSearchParams(location.search)
const waiting = Number(params.get('waiting') ?? '0')
const control = params.get('control') === '1'
const withDx = params.get('dx') !== '0'

document.documentElement.setAttribute('data-theme', params.get('theme') === 'light' ? 'light' : 'dark')
localStorage.clear()

const calls: string[] = []
;(window as unknown as { __calls: () => string[] }).__calls = () => [...calls]
const record = (name: string) => () => {
  calls.push(name)
}

const RADIO = {
  dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true,
  tuning: false, txAllowed: true, rigKeyed: false, holdTxFreq: false, txEven: true, txCycleAuto: true,
} as unknown as RadioStatus

const QSO = (withDx
  ? { state: 'AwaitRr73', dxcall: 'VP2V/W1AW', rxReport: -12, running: false, cqRunning: false,
      txNow: 'VP2V/W1AW KR4FQG R-07', txCount: 3 }
  : { state: 'CallingCq', dxcall: null, rxReport: null, running: true, cqRunning: true,
      txNow: 'CQ KR4FQG EM85', txCount: 1 }) as unknown as QsoStatus

function Probe() {
  useViewport(1)
  return (
    <div className="app" style={{ height: '100%' }}>
      {control && <style>{'.cq-log-waiting { position: static !important; display: inline-block; width: 900px; }'}</style>}
      <main className="layout single">
        <div className="probe-header" style={{ height: 56 }} />
        <OperateQsoStrip
          qso={QSO}
          radio={RADIO}
          onSetMode={record('set_mode')}
          onCallCq={record('call_cq')}
          onResend={record('resend')}
          onFreetext={record('freetext')}
          onLog={record('log')}
          logWaiting={waiting}
          onSetTxEnabled={record('set_tx_enabled')}
          onSetTune={record('set_tune')}
          onHaltTx={record('halt_tx')}
          onSetHoldTxFreq={record('hold_tx_freq')}
          onSetTxEven={record('tx_even')}
          onSetTxCycleAuto={record('tx_cycle_auto')}
          skipTx1={false}
          onSkipTx1={record('skip_tx1')}
          nextSlotSec={9}
        />
        <div className="cockpit-panes" data-cols={2} data-flow="fill" style={{ flex: '1 1 auto', minHeight: 0 }}>
          <div className="cockpit-col" />
          <div className="cockpit-col" />
        </div>
      </main>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
