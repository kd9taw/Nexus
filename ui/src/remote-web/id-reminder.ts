// The ID reminder (the audio design's M11, FCC §97.119): the operator identifies, and the page makes
// the interval visible. DISPLAY ONLY. There is no automatic identification anywhere in Nexus, and this
// is not one: it keys nothing, holds nothing and refuses nothing. It prompts.
//
// A RUN is a string of overs no more than ID_RUN_GAP_MS apart - a contact, as the rule counts it. The
// prompt is due ID_FIRST_MS into a run and every ID_EVERY_MS after, and it is SHOWN at the first
// moment the operator is on the air once it is due: that is when they can do something about it, and
// an operator who made one short over and then only listened is never nagged. It comes once more when
// the stream ends with a run in it, the end of the communication as far as this page can tell. Ten
// minutes is also the station's hard ceiling on one over, so no single over outruns the interval.

/** Nine minutes into a run: the prompt comes with a minute to spare. */
export const ID_FIRST_MS = 9 * 60_000
/** Then every ten minutes, the interval the rule sets. */
export const ID_EVERY_MS = 10 * 60_000
/** A pause longer than this ends a run; the next over starts a new one. */
export const ID_RUN_GAP_MS = 10 * 60_000

export class IdReminder {
  private runStart: number | null = null
  private lastOnAir: number | null = null
  private nextDue: number | null = null

  /** One look at the over: is it on the air now? Returns true when the prompt should show. */
  observe(now: number, onAir: boolean): boolean {
    if (this.lastOnAir !== null && !onAir && now - this.lastOnAir > ID_RUN_GAP_MS) this.forget()
    if (!onAir) return false
    if (this.runStart === null || this.nextDue === null) {
      this.runStart = now
      this.nextDue = now + ID_FIRST_MS
    }
    this.lastOnAir = now
    if (now < this.nextDue) return false
    // Counted from the run's own start, never from when the prompt happened to show, so a prompt
    // shown late does not push every later one later.
    while (this.nextDue <= now) this.nextDue += ID_EVERY_MS
    return true
  }

  /** The stream ended. Returns whether a run was in progress, which is the prompt's last moment. */
  end(): boolean {
    const had = this.runStart !== null
    this.forget()
    return had
  }

  private forget(): void {
    this.runStart = null
    this.lastOnAir = null
    this.nextDue = null
  }
}
