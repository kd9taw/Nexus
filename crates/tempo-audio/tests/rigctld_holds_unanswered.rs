//! A Hamlib whose capability tools could not be STARTED does not get a real port opened without
//! the RTS/DTR holds, even when the daemon itself starts a moment later (security review M-A,
//! 1.24.0).
//!
//! The moment is made real, not mocked. The only `rigctld` on PATH is a stand-in, and the first
//! time Nexus probes it (`--version`, while resolving the binary) it leaves a child holding its own
//! file open for writing for a short while. Every exec of it then fails `ETXTBSY` (Text file busy):
//! the `rigctl` beside it is missing and the `rigctld -L` ask cannot start, so no tool answered
//! at all. The resolve that follows retries `ETXTBSY` (`proc_util::runs_ok_within`) and gets it
//! back once the writer is gone, so the daemon launch WOULD start: before the fix it did, with no
//! `rts_state`/`dtr_state`, on a port whose driver raises both lines at open.
//!
//! In its own file because it sets the process-wide PATH.
#![cfg(all(target_os = "linux", feature = "serial"))]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use tempo_audio::rigctld_proc::{
    is_launch_refusal, spawn_rigctld, ControlLines, KeyingFacts, HAMLIB_CANNOT_CONFIRM_HOLDS,
};

/// The stand-in `rigctld`. Its first `--version` arms the busy window; `-L` asks are recorded
/// (one reaching it means the window was missed); any other launch is the daemon, recorded and
/// kept up.
const STAND_IN: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
d=$(dirname "$0")
case " $* " in
  *" --version "*)
    if [ ! -e "$d/armed" ]; then
      : > "$d/armed"
      ( exec 3>>"$0"; : > "$d/held"; sleep 0.2 ) </dev/null >/dev/null 2>&1 &
      while [ ! -e "$d/held" ]; do sleep 0.01; done
    fi
    echo "rigctld Hamlib 4.7.1 (stand-in)"
    exit 0;;
  *" -L "*) echo "$*" >> "$d/asked"; exit 0;;
esac
echo "$*" >> "$d/launches"
exec sleep 60
"#;

#[test]
fn a_capability_ask_no_tool_could_start_does_not_open_the_port_without_holds() {
    let real_port = format!("/dev/nexus-test-no-such-port-{}", std::process::id());
    // The busy window is a wall clock, so an attempt in which the `-L` ask still reached the
    // stand-in (a machine too loaded to ask within it) proves nothing and is run again.
    for attempt in 0..3 {
        let dir =
            std::env::temp_dir().join(format!("nexus-unanswered-{}-{attempt}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let bin = dir.join("rigctld");
        std::fs::File::create(&bin)
            .and_then(|mut f| f.write_all(STAND_IN.as_bytes()))
            .expect("write stand-in");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        // Only the stand-in: no other Hamlib may answer the ask, or be the daemon.
        std::env::set_var("PATH", &dir);

        let tcp = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .expect("free port");
        let outcome = spawn_rigctld(
            3073,
            &real_port,
            115_200,
            tcp,
            false,
            KeyingFacts::default(),
            ControlLines::hold_low(),
        );
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        let (asked, launches) = (read("asked"), read("launches"));
        if !asked.is_empty() {
            eprintln!("attempt {attempt}: the -L ask reached the stand-in ({asked:?}); again");
            drop(outcome);
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        let e = match outcome {
            Ok(proc_) => {
                // The daemon writes its argv as it starts; give it a moment, for the message.
                let mut launches = launches;
                for _ in 0..100 {
                    if !launches.is_empty() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    launches = read("launches");
                }
                drop(proc_);
                panic!(
                    "no Hamlib tool could start, and the daemon was launched anyway, without \
                     the holds: {launches:?}"
                )
            }
            Err(e) => e,
        };
        assert!(
            is_launch_refusal(&e),
            "refused by type: {e} ({:?})",
            e.kind()
        );
        assert_eq!(e.to_string(), HAMLIB_CANNOT_CONFIRM_HOLDS);
        assert!(
            launches.is_empty(),
            "a daemon was launched before the refusal: {launches:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    panic!("the busy window was missed on every attempt; the scenario never arose");
}
