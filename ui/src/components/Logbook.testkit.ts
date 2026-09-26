// THE LOGBOOK LIST'S LAST TIMER, for the suites that scroll it: `afterAll(waitOutTheListsScrollTimer)`.
//
// The list's end-of-scroll report comes 150 ms after the last scroll event, from a timer the list
// does not clear when it unmounts. Let the last test's land while the test file's window still
// exists (after it, React has no `window` to read, and the run fails on an error in no test).

/** Resolves once the list's end-of-scroll timer has had time to fire. */
export function waitOutTheListsScrollTimer(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 200))
}
