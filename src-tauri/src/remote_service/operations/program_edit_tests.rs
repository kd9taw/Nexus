//! Curating the working channel list from a browser: keyed by channel ID, checked against the
//! document revision the browser was shown, and written through the desktop's own writer.
use super::*;
use serde_json::json;

fn dir(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("remote-progedit-{tag}-{stamp}"));
    std::fs::create_dir(&dir).unwrap();
    dir
}

fn channel(i: usize) -> propagation::memchan::Channel {
    let mut c: propagation::memchan::Channel = serde_json::from_value(json!({"id":"seed",
        "name":"W1AW","rxMhz":146.94,"duplex":"minus","offsetMhz":0.6,"toneMode":"tone",
        "rtoneHz":100.0,"ctoneHz":100.0,"dtcsCode":23,"mode":"fm","comment":"Newington, CT",
        "dmrColorCode":null,"dmrTimeslot":null,"dmrTalkgroup":null,"dstarRpt1":null,
        "dstarRpt2":null,"source":null}))
    .unwrap();
    c.id = format!("manual:{i}");
    c.name = format!("CH{i:04}");
    c.rx_mhz = 146.0 + i as f64 / 1000.0;
    c
}

fn seed(path: &std::path::Path, id: &str, count: usize) {
    let file = crate::RadioProgFile {
        version: 1,
        projects: vec![crate::RadioProgProject {
            id: id.into(),
            name: "My channels".into(),
            created_utc: 1,
            updated_utc: 2,
            channels: (0..count).map(channel).collect(),
            ..Default::default()
        }],
    };
    std::fs::write(path, serde_json::to_vec(&file).unwrap()).unwrap();
}

/// The list as it stands, as (id, name) — what the browser would show next.
fn rows(path: &std::path::Path) -> Vec<(String, String)> {
    crate::remote_service::operations::program_export::radioprog(path)
        .unwrap()
        .unwrap()
        .projects
        .into_iter()
        .find(|p| p.id == crate::remote_service::operations::program_export::WORKING_PROJECT_ID)
        .unwrap()
        .channels
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect()
}

fn revision(path: &std::path::Path) -> String {
    crate::remote_service::query::programming_revision(path).unwrap()
}

fn applied(work: Result<ChangeWork, ChangeReason>) -> bool {
    matches!(work, Ok(ChangeWork::ProgramSaved))
}

#[test]
fn renames_moves_and_removes_a_row_by_its_id_and_never_by_a_position() {
    let dir = dir("curate");
    let path = dir.join("radioprog.json");
    seed(&path, "working", 4);
    assert_eq!(
        rows(&path).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["manual:0", "manual:1", "manual:2", "manual:3"]
    );
    // Rename the THIRD row by id. The name is the operator's own label, untruncated in the model.
    assert!(applied(prepare(
        &path,
        &revision(&path),
        &Edit::Rename {
            id: "manual:2".into(),
            name: "W1AW RPT".into()
        }
    )));
    assert_eq!(rows(&path)[2], ("manual:2".into(), "W1AW RPT".into()));
    // Move it up one place; the ids swap, the names ride along with them.
    assert!(applied(prepare(
        &path,
        &revision(&path),
        &Edit::Move {
            id: "manual:2".into(),
            by: -1
        }
    )));
    assert_eq!(
        rows(&path).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["manual:0", "manual:2", "manual:1", "manual:3"]
    );
    assert_eq!(rows(&path)[1].1, "W1AW RPT");
    // ...and down again, back where it was.
    assert!(applied(prepare(
        &path,
        &revision(&path),
        &Edit::Move {
            id: "manual:2".into(),
            by: 1
        }
    )));
    assert_eq!(
        rows(&path).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["manual:0", "manual:1", "manual:2", "manual:3"]
    );
    // Remove the FIRST row: every other row survives, including the renamed one.
    assert!(applied(prepare(
        &path,
        &revision(&path),
        &Edit::Remove {
            id: "manual:0".into()
        }
    )));
    assert_eq!(
        rows(&path),
        [
            ("manual:1".to_string(), "CH0001".to_string()),
            ("manual:2".to_string(), "W1AW RPT".to_string()),
            ("manual:3".to_string(), "CH0003".to_string())
        ]
    );
    // Clear empties the working list and leaves the project itself in place.
    assert!(applied(prepare(&path, &revision(&path), &Edit::Clear {})));
    assert_eq!(rows(&path), Vec::<(String, String)>::new());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ⛔ The whole point of keying by ID: a list that moved under the browser must not be edited at
/// the position the browser was looking at.
#[test]
fn a_change_against_a_stale_revision_touches_nothing() {
    let dir = dir("stale");
    let path = dir.join("radioprog.json");
    seed(&path, "working", 3);
    let shown = revision(&path);
    // Somebody at the shack removes a row. The browser's revision is now stale.
    assert!(applied(prepare(
        &path,
        &shown,
        &Edit::Remove {
            id: "manual:0".into()
        }
    )));
    let after = rows(&path);
    assert_eq!(
        prepare(
            &path,
            &shown,
            &Edit::Remove {
                id: "manual:1".into()
            }
        )
        .err(),
        Some(ChangeReason::ContextChanged)
    );
    assert_eq!(rows(&path), after, "a refused change still wrote the file");
    // POSITIVE CONTROL: the same change against the CURRENT revision applies, so the refusal above
    // was the revision check and not a change that could never work.
    assert!(applied(prepare(
        &path,
        &revision(&path),
        &Edit::Remove {
            id: "manual:1".into()
        }
    )));
    assert_eq!(
        rows(&path).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["manual:2"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_row_that_is_not_there_and_a_move_off_the_end_are_refused() {
    let dir = dir("missing");
    let path = dir.join("radioprog.json");
    seed(&path, "working", 2);
    let before = rows(&path);
    for edit in [
        Edit::Rename {
            id: "manual:9".into(),
            name: "X".into(),
        },
        Edit::Remove {
            id: "manual:9".into(),
        },
        Edit::Move {
            id: "manual:9".into(),
            by: 1,
        },
        // Off the top and off the bottom of a two-row list.
        Edit::Move {
            id: "manual:0".into(),
            by: -1,
        },
        Edit::Move {
            id: "manual:1".into(),
            by: 1,
        },
    ] {
        assert_eq!(
            prepare(&path, &revision(&path), &edit).err(),
            Some(ChangeReason::InvalidChange)
        );
        assert_eq!(rows(&path), before, "a refused change wrote the file");
    }
    // POSITIVE CONTROL: a move that IS in range applies against the same fixture.
    assert!(applied(prepare(
        &path,
        &revision(&path),
        &Edit::Move {
            id: "manual:0".into(),
            by: 1
        }
    )));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file with no working list, and a file that cannot be read at all. Neither may be "fixed" by
/// writing a fresh empty list over it — that is the `load_radioprog` trap, and here it would
/// destroy the operator's named projects rather than just show a blank screen.
#[test]
fn an_unreadable_or_absent_list_is_never_written_over() {
    let dir = dir("absent");
    let path = dir.join("radioprog.json");
    // A file holding only a NAMED project: the revision is real, but there is no working list.
    seed(&path, "denver-trip", 3);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        prepare(
            &path,
            &revision(&path),
            &Edit::Remove {
                id: "manual:0".into()
            }
        )
        .err(),
        Some(ChangeReason::InvalidChange)
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Unreadable bytes: the revision cannot even be computed, so it cannot match.
    std::fs::write(&path, b"{ not json").unwrap();
    assert_eq!(
        prepare(&path, &"a".repeat(64), &Edit::Clear {}).err(),
        Some(ChangeReason::ContextChanged)
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
    // POSITIVE CONTROL: a readable file with a working list does take the same Clear.
    seed(&path, "working", 3);
    assert!(applied(prepare(&path, &revision(&path), &Edit::Clear {})));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_wire_grammar_is_closed() {
    assert!(Edit::Clear {}.valid());
    assert!(Edit::Rename {
        id: "manual:0".into(),
        name: String::new()
    }
    .valid());
    assert!(Edit::Rename {
        id: "manual:0".into(),
        name: "a".repeat(1024)
    }
    .valid());
    // An id or name no `programming` document could have carried, a newline smuggled into a CSV
    // field, and a move that is not one place.
    for edit in [
        Edit::Rename {
            id: String::new(),
            name: "X".into(),
        },
        Edit::Rename {
            id: "a".repeat(257),
            name: "X".into(),
        },
        Edit::Rename {
            id: "manual:0".into(),
            name: "a".repeat(1025),
        },
        Edit::Rename {
            id: "manual:0".into(),
            name: "W1AW\nEXTRA,ROW".into(),
        },
        Edit::Remove { id: String::new() },
        Edit::Move {
            id: "manual:0".into(),
            by: 0,
        },
        Edit::Move {
            id: "manual:0".into(),
            by: 2,
        },
    ] {
        assert!(!edit.valid(), "{}", serde_json::to_string(&edit).unwrap());
    }
    // Nothing else may ride along: no path, no project, no whole list of channels.
    for raw in [
        json!({"action":"clear","path":"/tmp/x.json"}),
        json!({"action":"remove","id":"manual:0","index":0}),
        json!({"action":"replace","channels":[]}),
        json!({"action":"rename","id":"manual:0"}),
    ] {
        assert!(
            serde_json::from_value::<Edit>(raw.clone()).is_err(),
            "{raw} was admitted"
        );
    }
    // POSITIVE CONTROL for that loop: the shapes the browser really sends do parse.
    for raw in [
        json!({"action":"clear"}),
        json!({"action":"remove","id":"manual:0"}),
        json!({"action":"move","id":"manual:0","by":-1}),
        json!({"action":"rename","id":"manual:0","name":"W1AW"}),
    ] {
        assert!(
            serde_json::from_value::<Edit>(raw.clone()).is_ok(),
            "{raw} was refused"
        );
    }
}
