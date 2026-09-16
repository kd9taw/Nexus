//! Two Nexus INSTANCES saving `settings.json` at the same time.
//!
//! The operator runs a tester build (`1.13.0-test1`) beside the public release against the same
//! data directory, so this is not a hypothetical: two processes, no single-instance guard, one
//! file. `Settings::save` writes a scratch file and renames it onto the target, and the rename is
//! what makes a save all-or-nothing — but only while the two instances are not writing into the
//! SAME scratch file. On a fixed `settings.json.tmp` they share one inode: each `open` truncates
//! it under the other, each `write_all` lands at its own offset, and the rename publishes the
//! mixture. The published file is then not a half-configuration but invalid JSON, which
//! `Settings::load` sets aside as `.corrupt` before starting from defaults — blanking the
//! operator's identity and rig config and resetting `license_class` to `Open`, which drops the
//! Part 97 TX lockout.
//!
//! This is an integration test rather than two threads in one process on purpose: threads share a
//! PID, and the per-process scratch name is precisely the thing under test. The children are this
//! same test binary, re-executed at the `#[ignore]`d worker below.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempo_app::settings::Settings;

/// The settings file each worker hammers.
const PATH_VAR: &str = "NEXUS_CONCURRENT_SAVE_PATH";
/// The worker's identity — it writes this as `mycall` and pads `op_name` to a length derived
/// from it, so competing saves produce files of very DIFFERENT byte lengths. Equal-length writes
/// into a shared scratch file can interleave and still come out as valid JSON by luck; unequal
/// ones leave a hole or a tail and cannot.
const CALL_VAR: &str = "NEXUS_CONCURRENT_SAVE_CALL";

const SAVES_PER_WORKER: usize = 60;
const WORKERS: usize = 3;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tempo_concurrent_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Load, change something, save — the ordinary session shape, `SAVES_PER_WORKER` times.
fn hammer(path: &Path, call: &str) {
    for i in 0..SAVES_PER_WORKER {
        let mut s = Settings::load(path);
        s.mycall = call.to_string();
        s.op_name = call.repeat(call.len() * 3);
        s.fd_position_name = format!("{call}-{i}");
        s.save(path).expect("saves");
    }
}

/// The child process. `#[ignore]`d so an ordinary run never executes it; the parent re-runs this
/// binary with `--exact --ignored` to reach it.
#[test]
#[ignore = "re-executed as a child process by the concurrent-save test"]
fn concurrent_save_worker() {
    let path = PathBuf::from(std::env::var(PATH_VAR).expect("worker needs a settings path"));
    let call = std::env::var(CALL_VAR).expect("worker needs a call");
    hammer(&path, &call);
}

#[test]
fn two_instances_saving_at_once_never_publish_a_torn_settings_file() {
    let dir = scratch_dir("save");
    let path = dir.join("settings.json");

    // Seed the file the way a NEWER build would have left it: everything this build knows, plus
    // a key it does not. That key is the payload — it has to survive every one of the
    // WORKERS * SAVES_PER_WORKER saves below, from whichever instance wins each rename.
    let mut seed = serde_json::to_value(Settings::default()).expect("serialises");
    let obj = seed.as_object_mut().expect("an object");
    obj.insert("futureKnobHz".into(), serde_json::json!(1234));
    std::fs::write(&path, serde_json::to_string_pretty(&seed).unwrap()).unwrap();

    let exe = std::env::current_exe().expect("this test binary");
    let mut children: Vec<_> = (0..WORKERS)
        .map(|n| {
            Command::new(&exe)
                .args([
                    "concurrent_save_worker",
                    "--exact",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env(PATH_VAR, &path)
                .env(CALL_VAR, format!("W{n}{}", "X".repeat(n * 4)))
                .spawn()
                .expect("spawn a second instance")
        })
        .collect();

    // …and this process is a fourth writer, so the parent is in the race too.
    hammer(&path, "KD9TAW");

    for (n, child) in children.iter_mut().enumerate() {
        let status = child.wait().expect("child runs");
        assert!(status.success(), "worker {n} failed: {status}");
    }

    // 1. Nothing torn was ever PUBLISHED. `Settings::load` renames an unparseable file aside as
    //    `settings.json.corrupt`, so one of those existing is the fingerprint of a torn read —
    //    and every worker did `SAVES_PER_WORKER` loads, so the whole run was watched, not just
    //    the end state. See `a_torn_settings_file_really_does_leave_a_corrupt_sibling` for the
    //    positive control: this check DOES fire when a torn file reaches load().
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "settings.json")
        .collect();
    assert!(
        leftovers.is_empty(),
        "the concurrent run left {leftovers:?} beside settings.json — a `.corrupt` means a torn \
         file was published and read; a `.tmp` means two instances collided on one scratch path"
    );

    // 2. The published file is a whole, valid configuration — not a mixture of two.
    let raw = std::fs::read_to_string(&path).unwrap();
    let back: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("published file is valid JSON: {e}"));
    let s = Settings::load(&path);
    let winners: Vec<String> = (0..WORKERS)
        .map(|n| format!("W{n}{}", "X".repeat(n * 4)))
        .chain(["KD9TAW".to_string()])
        .collect();
    assert!(
        winners.contains(&s.mycall),
        "mycall is one writer's whole value, not a splice: {:?}",
        s.mycall
    );
    // Last-writer-wins is the accepted outcome for settings, and this is what it means: every
    // field in the file came from the SAME save, so the operator sees one instance's complete
    // configuration rather than two instances' fields interleaved.
    assert_eq!(
        s.op_name,
        s.mycall.repeat(s.mycall.len() * 3),
        "every field came from one save — mycall and op_name agree"
    );
    assert!(
        s.fd_position_name.starts_with(&s.mycall),
        "…and so did the third field: {:?} vs {:?}",
        s.fd_position_name,
        s.mycall
    );

    // 3. THE PAYLOAD. The newer build's key survived ~240 concurrent saves by four instances.
    assert_eq!(
        back.get("futureKnobHz"),
        Some(&serde_json::json!(1234)),
        "the key no instance understands was written out of existence"
    );
    // POSITIVE CONTROL for that lookup: the same `get` must report a key that is not there.
    assert_eq!(
        back.get("aKeyNobodyEverWrote"),
        None,
        "the control key must be absent, or the assertion above cannot fail"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// POSITIVE CONTROL for check 1 above. "No `.corrupt` file appeared" is worth nothing until the
/// same check is shown catching a torn file that really did reach `Settings::load` — otherwise a
/// green run only proves that load() never sets anything aside, which would make the concurrent
/// test unfalsifiable.
#[test]
fn a_torn_settings_file_really_does_leave_a_corrupt_sibling() {
    let dir = scratch_dir("control");
    let path = dir.join("settings.json");
    Settings::default().save(&path).unwrap();
    // Exactly what a shared scratch file publishes: one save's head, another's tail.
    let torn = {
        let whole = std::fs::read_to_string(&path).unwrap();
        format!("{}{}", &whole[..whole.len() / 2], &whole[..40])
    };
    std::fs::write(&path, &torn).unwrap();

    let _ = Settings::load(&path);

    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "settings.json")
        .collect();
    assert_eq!(
        leftovers,
        vec!["settings.json.corrupt".to_string()],
        "a torn file DOES leave the sibling the concurrent test asserts the absence of"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
