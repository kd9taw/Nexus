// JS8Call's notification sounds (Settings ▸ Notifications, every one off by default,
// Configuration.cpp:1996-2002, :2464): a sound when a CQ, a heartbeat, an ACK for me, a message
// directed to me, a message for my inbox, a new callsign or a callsign I have worked is heard. JS8Call
// plays a sound file the operator picks for each (`tryNotify`, mainwindow.cpp:5595-5603), and so does
// Nexus where one is picked (Settings `js8SoundFiles`); where none is, it plays a tone of its own for
// each, through the same WebAudio beep its other alerts use (alerts.ts), so no two events sound alike.
// A file that cannot be played plays that tone instead and is named once: JS8Call plays nothing then
// (NotificationAudio.cpp:72-74).
//
// What is heard is read off polls of the engine (the feed's new frames, the inbox and the heard list;
// Js8SoundPlayer, from whichever screen is showing), as JS8Call's own triggers read its decodes: `tryNotify("cq")` and ("hb") on a
// heartbeat frame (processDecodeEvent.cpp:407, :429), ("directed") on a command addressed to me or a
// group I joined (processCommandActivity.cpp:266-277), ("ack") on an ACK to me (:849), ("inbox") when a
// message is filed for me (:587, :806, :823), ("call_new"/"call_old") when a call first enters the
// call list, by whether the log has it (mainwindow.cpp:2313-2318). The first poll only learns what is
// already there: nothing heard before the watch started sounds.
import type { Js8ActivityRow, Js8Heard, Js8InboxEntry } from '../types'
import { addresseeOf } from './js8Lists'
import { js8BaseCall } from '../js8Vocab'
import { beep, doubleBeep, ensureCtx } from '../alerts'
import { readJs8SoundFile } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'

export const JS8_SOUND_EVENTS = ['cq', 'hb', 'ack', 'directed', 'inbox', 'callNew', 'callOld'] as const
export type Js8SoundEvent = (typeof JS8_SOUND_EVENTS)[number]

const rowKey = (r: Js8ActivityRow): string => `${r.atMs}|${r.freqHz}|${r.mine ? 1 : 0}|${r.text}`

/** What one frame announces, if anything. */
export function js8FrameEvent(r: Js8ActivityRow, myCall: string, groups: readonly string[]): Js8SoundEvent | null {
  if (r.mine || !r.from || r.flags?.assembled) return null
  const text = r.text.toUpperCase()
  if (/: @ALLCALL CQ\b/u.test(text)) return 'cq'
  if (/: @HB HEARTBEAT\b/u.test(text)) return 'hb'
  if (!r.directedToMe) return null
  const to = addresseeOf(text)
  const me = myCall.trim().toUpperCase()
  const mine = to.startsWith('@') ? to !== '@ALLCALL' && groups.some((g) => g.trim().toUpperCase() === to) : me !== '' && js8BaseCall(to) === js8BaseCall(me)
  if (!mine) return null
  return /^[^:\s]+:\s+\S+\s+ACK\b/u.test(text) ? 'ack' : 'directed'
}

/**
 * Remembers what the cockpit has already seen, poll to poll, and says what is new. A frame is new
 * once; a call is new once, and is announced only when the log has said whether it is worked
 * (`worked` answers null until then), so the right one of the two sounds plays.
 */
export class Js8SoundWatch {
  private rows: Set<string> | null = null
  private calls: Set<string> | null = null
  private waiting = new Set<string>()
  private filed: Set<number> | null = null

  /** The events among `rows` not seen before; the first call only learns them. */
  frames(rows: readonly Js8ActivityRow[], myCall: string, groups: readonly string[]): Js8SoundEvent[] {
    const keys = rows.map(rowKey)
    const seen = this.rows
    this.rows = new Set(keys)
    if (seen === null) return []
    const out: Js8SoundEvent[] = []
    rows.forEach((r, i) => {
      if (seen.has(keys[i])) return
      const ev = js8FrameEvent(r, myCall, groups)
      if (ev) out.push(ev)
    })
    return out
  }

  /** "inbox" when a message for me was filed since the last call (an unread entry not seen before);
   *  the first call only learns what is there. */
  inbox(entries: readonly Js8InboxEntry[]): Js8SoundEvent[] {
    const seen = this.filed
    this.filed = new Set(entries.map((e) => e.id))
    if (seen === null) return []
    return entries.some((e) => !seen.has(e.id) && e.state === 'unread') ? ['inbox'] : []
  }

  /** "callNew" or "callOld" for each call heard for the first time, once the log has answered for it. */
  newCalls(stations: readonly Js8Heard[], worked: (call: string) => boolean | null): Js8SoundEvent[] {
    const now = stations.map((h) => h.call)
    if (this.calls === null) {
      this.calls = new Set(now)
      return []
    }
    for (const c of now) {
      if (!this.calls.has(c)) {
        this.calls.add(c)
        this.waiting.add(c)
      }
    }
    const out: Js8SoundEvent[] = []
    for (const c of [...this.waiting]) {
      const w = worked(c)
      if (w === null) continue
      this.waiting.delete(c)
      out.push(w ? 'callOld' : 'callNew')
    }
    return out
  }
}

/** Each event's tone (Hz), apart from the other alerts' (alerts.ts BEEP_HZ, the 990 Hz alarm, the
 *  660/880 TX pair): a message for me rings twice. */
const TONE: Record<Js8SoundEvent, number> = {
  cq: 587,
  hb: 466,
  ack: 1046,
  directed: 932,
  inbox: 784,
  callNew: 698,
  callOld: 554,
}

/** One event's built-in tone. */
function playTone(ev: Js8SoundEvent): void {
  if (ev === 'inbox' || ev === 'directed') doubleBeep(TONE[ev])
  else beep(TONE[ev])
}

/** The sound files read and decoded this session, by path: JS8Call reads each once too
 *  (NotificationAudio's cache, NotificationAudio.cpp:67-70, :105). */
const clips = new Map<string, AudioBuffer>()
/** The event and file pairs already named as not playing. */
const told = new Set<string>()

/** For tests: forget the files read and the failures named. */
export function __resetJs8SoundsForTest(): void {
  clips.clear()
  told.clear()
}

/** A path's last part, the file's name, as the Settings row and the notice show it. */
export function js8SoundFileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

/** Why a file did not play, in the operator's words: the station's refusal by name
 *  (`src-tauri/src/js8_sounds.rs`), or a file this computer could not decode. */
function whyNot(reason: unknown): string {
  switch (reason) {
    case 'missing':
      return t('js8.sound.file.missing')
    case 'unreadable':
      return t('js8.sound.file.unreadable')
    case 'tooLarge':
      return t('js8.sound.file.tooLarge')
    case 'notWav':
      return t('js8.sound.file.notWav')
    default:
      return t('js8.sound.file.undecodable')
  }
}

/**
 * Play one event's sound: its WAV file when one is set (Settings `js8SoundFiles`), else its built-in
 * tone. A file that cannot be played (gone, unreadable, too large, not a WAV, or not decodable here)
 * plays the tone instead and is named once per event and file; `always` names it every time, for
 * Settings' Test. Through the alerts' own audio, never the radio's.
 */
export async function playJs8Sound(ev: Js8SoundEvent, file?: string | null, always = false): Promise<void> {
  const path = file?.trim() ?? ''
  const ctx = path ? ensureCtx() : null
  if (ctx) {
    try {
      let clip = clips.get(path)
      if (!clip) {
        clip = await ctx.decodeAudioData(await readJs8SoundFile(path))
        clips.set(path, clip)
      }
      const source = ctx.createBufferSource()
      source.buffer = clip
      source.connect(ctx.destination)
      source.start()
      return
    } catch (reason) {
      const key = `${ev}\n${path}`
      if (always || !told.has(key)) {
        told.add(key)
        pushToast(t('js8.sound.file.failed', { file: js8SoundFileName(path), reason: whyNot(reason) }), 'error', 12_000)
      }
    }
  }
  playTone(ev)
}

/** The events of one poll to play, each once, in JS8Call's table order, the enabled ones only. */
export function js8SoundsToPlay(events: readonly Js8SoundEvent[], enabled: readonly string[]): Js8SoundEvent[] {
  return JS8_SOUND_EVENTS.filter((ev) => events.includes(ev) && enabled.includes(ev))
}
