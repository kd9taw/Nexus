//! A Hamlib whose capability tools could not be STARTED does not get a real port opened without
//! the RTS/DTR holds, even when the daemon itself starts a moment later (security review M-A,
//! 1.24.0).
//!
//! The moment is made real, not mocked. The only `rigctld` on PATH is a stand-in, and the first
//! time Nexus probes it (`--version`, while resolving the binary) it leaves a child holding its own
//! file open for writing. Every exec of it then fails `ETXTBSY` (Text file busy): the `rigctl`
//! beside it is missing and the `rigctld -L` ask cannot start, so no tool answered at all.
//!
//! ⚠️ THE WINDOW ENDS ON THE TEST'S WORD, NOT A CLOCK. The ask retries a busy tool a few times,
//! briefly (`ask_show_conf`: 5 more tries 50 ms apart), so a window of a fixed length either
//! ended inside those retries on a slow machine (the ask then reached the stand-in and the
//! scenario never arose) or had to be guessed long enough for every machine. The writer holds
//! until the test has `spawn_rigctld`'s answer, so every one of the ask's tries meets a busy
//! file, on any machine; it gives up by itself after 30 s only so a crashed test leaves nothing.
//! Before the fix, no tool answering claimed nothing and the launch went on to open the port with
//! no `rts_state`/`dtr_state`, on a port whose driver raises both lines at open; here that launch
//! meets the same busy file, and either way its answer is not the refusal this test asks for.
//!
//! In its own file because it sets the process-wide PATH.
#![cfg(all(target_os = "linux", feature = "serial"))]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use tempo_audio::rigctld_proc::{
    is_launch_refusal, spawn_rigctld, ControlLines, KeyingFacts, HAMLIB_CANNOT_CONFIRM_HOLDS,
};

/// The stand-in `rigctld`. Its first `--version` arms the busy window, held until `release`
/// appears beside it; `-L` asks are recorded (none may reach it); any other launch is the daemon,
/// recorded and kept up.
const STAND_IN: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
d=$(dirname "$0")
case " $* " in
  *" --version "*)
    if [ ! -e "$d/armed" ]; then
      : > "$d/armed"
      ( exec 3>>"$0"; : > "$d/held"; i=0
        while [ ! -e "$d/release" ] && [ $i -lt 3000 ]; do sleep 0.01; i=$((i+1)); done
      ) </dev/null >/dev/null 2>&1 &
      while [ ! -e "$d/held" ]; do sleep 0.01; done
    fi
    echo "rigctld Hamlib 4.7.1 (stand-in)"
    exit 0;;
  *" -L "*) echo "$*" >> "$d/asked"; exit 0;;
esac
echo "$*" >> "$d/launches"
exec sleep 60
"#;

/// The kernel premise: exec of a file open for writing fails `ETXTBSY`. Linux has refused it for
/// as long as Nexus has run there; a kernel that lets it run cannot make this moment at all.
fn exec_of_a_busy_file_is_refused(dir: &std::path::Path) -> bool {
    let probe = dir.join("probe");
    std::fs::write(&probe, "#!/bin/sh\n").expect("write probe");
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let writer = std::fs::OpenOptions::new()
        .append(true)
        .open(&probe)
        .expect("open probe");
    let ran = std::process::Command::new(&probe).status();
    drop(writer);
    matches!(ran, Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy)
}

#[test]
fn a_capability_ask_no_tool_could_start_does_not_open_the_port_without_holds() {
    let real_port = format!("/dev/nexus-test-no-such-port-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("nexus-unanswered-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    if !exec_of_a_busy_file_is_refused(&dir) {
        eprintln!("skipped: this kernel runs a file open for writing, so no tool can be kept busy");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
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
    // The answer is in: the busy window may end now.
    std::fs::write(dir.join("release"), "").expect("release the writer");
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    assert!(
        dir.join("armed").exists(),
        "premise: the resolve's --version armed the busy window"
    );
    let asked = read("asked");
    assert!(
        asked.is_empty(),
        "the -L ask reached the stand-in through a busy window: {asked:?}"
    );
    let e = match outcome {
        Ok(proc_) => {
            // The daemon writes its argv as it starts; give it a moment, for the message.
            let mut launches = read("launches");
            for _ in 0..100 {
                if !launches.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
                launches = read("launches");
            }
            drop(proc_);
            panic!(
                "no Hamlib tool could start, and the daemon was launched anyway, without the \
                 holds: {launches:?}"
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
    let launches = read("launches");
    assert!(
        launches.is_empty(),
        "a daemon was launched before the refusal: {launches:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
