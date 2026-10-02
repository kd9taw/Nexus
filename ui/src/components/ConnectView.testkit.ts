// What a Conditions test about anything else needs since step 5: its surfaces past the one-time switch to
// Frame + bar (ConnectView `switchToDefaultOnce`), so the view opens on the layout the test's own records
// describe — Standard when it seeds none, which is where an operator who has updated and chosen Standard
// is. Without it, a test that renders from empty storage is a fresh install, and opens in Frame + bar.
// The switch itself is ConnectView.switch.test.tsx's.

/** Mark these surfaces past the switch (default: the main window and the dashboard window), keeping nothing. */
export function pastTheSwitch(...surfaces: string[]): void {
  for (const surface of surfaces.length ? surfaces : ['main', 'connect'])
    localStorage.setItem(`nexus.connect.switch.${surface}`, JSON.stringify({ v: 1, kept: null }))
}
