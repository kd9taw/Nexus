# Tempo UI — Buckets B (work-a-station + logbook) + C (alerts + comforts)

## Contract
- [x] types.ts: Station.worked; DecodeRow; AppSnapshot.recentDecodes; LoggedQso; Settings(autoLog, alertMyCall, alertCq, alertNew, macros)
- [x] api.ts: callStation, logQso, getLog (invoke + mock)
- [x] mock.ts: recentDecodes seed (cq/directedToMe/worked); some stations worked; getLog ~3; callStation->qso S&P w/ dxcall; logQso appends + marks worked; default settings autoLog/alerts/macros; advance() rolls decode rows

## Bucket B
- [x] StationCard: split into clickable open-area + Work button -> callStation; B4 chip + worked gray; bearing+dist
- [x] QsoPanel: reflects qso.dxcall ("Working <dxcall> (S&P)" after callStation)
- [x] Logbook view: getLog() table (call/band/freq/mode/sent/rcvd/UTC) — new component + ModeNav entry
- [x] Log QSO manual form (inline, toggled) -> logQso
- [x] autoLog toggle in Settings (Operating section)

## Bucket C
- [x] DecodeFeed component: recentDecodes color-coded (cq accent / directedToMe strong / worked gray+B4 / new subtle); Work button per row; in right rail across views
- [x] roster StationCard color-coded (worked gray+B4)
- [x] alerts.ts: WebAudio beep + toast, gated by settings, dedup by from+msg+freq, session new-station set; alert toggles in Settings
- [x] UTC clock in TopBar (live, HH:MM:SS)
- [x] grid.ts bearingLabel(); shown in StationCard next to distance
- [x] Editable macros: Composer + BandFeed read settings.macros; macro editor in Settings; Field Day exchange stays dynamic

## Verify
- [x] npm run build green (tsc -b --force exit 0; vite OK); previews via mock; look/themes preserved

## Review
- New files: src/alerts.ts, src/components/DecodeFeed.tsx, src/components/Logbook.tsx.
- StationCard is now a container (open-button + Work-button) — valid HTML, B4 + worked styling, bearing.
- App holds a settings copy (macros + alert gating); a useEffect feeds recentDecodes to processDecodes.
- callStation enters QSO S&P targeting the call and jumps to the QSO view.
- Logbook is a distinct ADIF view (📖) separate from the Field Log (📋); manual Log QSO form posts logQso.
- Mock rolls a fresh decode each RX slot (~45%) incl. new/CQ/directed rows so the feed + alerts are alive.
- Build: CSS 32.7 -> 36.6 kB; JS 206.2 -> 222.2 kB (68.5 kB gz).
