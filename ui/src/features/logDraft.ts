// THE LOG STRIP'S DRAFT — the contact a cockpit's log strip held when the operator left that
// cockpit for another page, kept so the strip shows it again on the way back (a user, 2026-10-14:
// "while waiting for a possibility to work a DX station, I also switch between the Nexus pages.
// However, if I then have to return to, say, the Voice Cockpit, I have to re-enter the call").
//
// WHY A STORE AND NOT A KEEP-ALIVE HOST. App draws the Phone and CW cockpits, and the Satellites
// section, in its view switch, so a page switch unmounts them, and `LogEntry` holds every field in
// its own state. The keyboard cockpits (RTTY, PSK, JS8) never lost their strip: their keep-alive
// hosts never unmount it. Phone and CW are deliberately not kept alive on the desktop: their
// unmount is part of their transmit cleanup (Phone's unkeys the rig and lets go of the microphone),
// and their window keys stop with it. So the contact stays, here, and the cockpit goes.
//
// IN MEMORY ONLY, ONE PER COCKPIT. A draft lasts as long as this window does — never across a
// restart, never in storage — and `LogEntry`'s `draft` prop names whose it is, so Voice's never
// shows in CW. What a draft holds, and when a strip keeps none, is `LogEntry`'s to say (its draft
// effect): this module keeps what it is handed and nothing else.
import type { StateSource } from '../api'

/** A contact as its strip showed it, and what each part of it belongs to. */
export interface LogDraft {
  call: string
  /** The two reports as set for this contact, or null for a box nobody has touched. */
  rstSent: string | null
  rstRcvd: string | null
  name: string
  qth: string
  comment: string
  notes: string
  grid: string
  state: string
  /** Where the state came from, which the station reads as the contact is logged. */
  stateSource: StateSource
  callbookState: string
  country: string
  image: string | null
  parkProgram: string
  parkRef: string
  /** Whose park the box holds, and whether a hunt put it there. */
  parkFor: { call: string; hunt: boolean }
  /** The park a Needed click handed over with its station. */
  workPark: { call: string; program: string; reference: string } | null
  /** The call the callbook's answer was for. */
  enrichedFor: string
  /** When the call was entered (Unix seconds), the contact's start; null when not known. */
  enteredUnix?: number | null
}

const drafts = new Map<string, LogDraft>()

/** The draft a cockpit's strip left, if it left one. */
export function readLogDraft(key: string): LogDraft | undefined {
  return drafts.get(key)
}

/** Keep `draft` as the cockpit's, or, given null, keep none. */
export function writeLogDraft(key: string, draft: LogDraft | null): void {
  if (draft) drafts.set(key, draft)
  else drafts.delete(key)
}

/** Tests: no drafts, after every test (src/test-setup.ts). */
export function __resetLogDraftsForTests(): void {
  drafts.clear()
}
;(globalThis as { __nexusTestResets?: Set<() => void> }).__nexusTestResets?.add(__resetLogDraftsForTests)
