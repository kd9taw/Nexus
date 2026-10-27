//! A shipped build never carries the bench's road switch (`crate::bench_road_switch`, the
//! `bench-road-switch` feature). These tests hold both halves of that down.
//!
//! 1. **The release never turns it on.** A cargo feature reaches a build only through the build
//!    command (`--features`, Tauri's `-f`, or asking for every feature), through another feature
//!    or the default set in `Cargo.toml`, through a Tauri config's `build.features`, or through a
//!    cfg a build script or a cargo config sets. [`refusals`] reads every file a shipped build is
//!    made from, as the release runs them, and names each place that would turn the switch on:
//!    - the release and publish workflows, and every script they name, followed through the
//!      scripts those name, never name the feature, but for `build-windows-cross.sh`'s comments
//!      and its own `--bench-road-switch` option: the option a test build passes and the release
//!      never can, since passing it would name the feature in a workflow;
//!    - none of them asks for every feature;
//!    - src-tauri's `Cargo.toml` declares the feature once, turning on the client core's switch and
//!      nothing else, and the client core's manifest declares its own once, enabling nothing;
//!      neither names it anywhere else, and no other crate's manifest names it, so no other
//!      feature, no default and no dependency turns it on;
//!    - no Tauri config, build script or cargo config names it.
//!
//!    It runs over this tree, which must give nothing, and over scratch copies of the same files,
//!    one way of turning the switch on planted in each, which it must name: the same function at
//!    another root, so a guard that read nothing could not pass.
//! 2. **A build without the feature has none of the switch**: no class, so both ends would refuse
//!    the command; no handler; client mode refuses it with nothing run; and the banner's status
//!    names no switch, so the screen draws none.
use std::path::{Path, PathBuf};

/// The feature, as every build command spells it.
const FEATURE: &str = "bench-road-switch";
/// Where the release's builds begin. Every script they name is read too, and every script those
/// name.
const WORKFLOWS: [&str; 2] = [
    ".github/workflows/release.yml",
    ".github/workflows/publish.yml",
];
/// The script a test build passes the feature's option to, and the option, as its case arm reads.
const CROSS: &str = "scripts/build-windows-cross.sh";
const OPTION: &str = "--bench-road-switch)";
/// The manifests that declare the feature, and each one's one declaration: src-tauri's, which turns
/// on the client core's switch and nothing else, and the client core's, which enables nothing.
const MANIFEST: &str = "src-tauri/Cargo.toml";
const CORE: &str = "crates/tempo-client/Cargo.toml";
const DECLARATIONS: [(&str, &str); 2] = [
    (
        MANIFEST,
        "bench-road-switch = [\"tempo-client/bench-road-switch\"]",
    ),
    (CORE, "bench-road-switch = []"),
];
/// What else src-tauri's build reads a cfg from, when it is there.
const BUILD_FILES: [&str; 3] = [
    "src-tauri/build.rs",
    ".cargo/config.toml",
    "src-tauri/.cargo/config.toml",
];

/// The repository's root: the parent of this crate's own directory.
fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri sits in the repository")
        .to_path_buf()
}

/// The scripts a line names: `scripts/<path>`, a full stop after one taken as the sentence's.
fn scripts_named(line: &str) -> Vec<String> {
    let mut named = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("scripts/") {
        let path: String = rest[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
            .collect();
        rest = &rest[at + path.len()..];
        let path = path.trim_end_matches('.');
        if path.len() > "scripts/".len() {
            named.push(path.to_string());
        }
    }
    named
}

/// The Tauri configs src-tauri has: `tauri.conf.json` and every platform's or flavour's beside it,
/// in any of the forms Tauri reads.
fn tauri_configs(root: &Path) -> Vec<String> {
    let mut configs: Vec<String> = std::fs::read_dir(root.join("src-tauri"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            (name.starts_with("tauri.") && (name.ends_with(".json") || name.ends_with(".json5")))
                || (name.starts_with("Tauri") && name.ends_with(".toml"))
        })
        .map(|name| format!("src-tauri/{name}"))
        .collect();
    configs.sort();
    configs
}

/// What would turn the switch on in a shipped build made from the tree at `root`: one line for each
/// place, naming its file and line. Empty for a tree whose shipped builds never carry it. `read`
/// gets every file it read, for the caller to check they are the files meant.
fn refusals(root: &Path, read: &mut Vec<String>) -> Vec<String> {
    let mut found = Vec::new();
    let mut waiting: Vec<String> = WORKFLOWS.iter().map(|file| file.to_string()).collect();
    while let Some(file) = waiting.pop() {
        if read.contains(&file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            // A workflow is always there; a name in prose that is no file is not a script.
            if WORKFLOWS.contains(&file.as_str()) {
                found.push(format!("{file}: not there"));
            }
            continue;
        };
        read.push(file.clone());
        for (n, line) in text.lines().enumerate() {
            let at = format!("{file}:{}", n + 1);
            if line.contains("--all-features") {
                found.push(format!("{at}: asks for every feature"));
            }
            let opts_in = file == CROSS && {
                let line = line.trim_start();
                line.starts_with('#') || line.starts_with(OPTION)
            };
            if line.contains(FEATURE) && !opts_in {
                found.push(format!("{at}: names {FEATURE}"));
            }
            waiting.extend(scripts_named(line));
        }
    }
    for (manifest, declaration) in DECLARATIONS {
        match std::fs::read_to_string(root.join(manifest)) {
            Err(_) => found.push(format!("{manifest}: not there")),
            Ok(text) => {
                read.push(manifest.to_string());
                let mut declared = 0;
                for (n, line) in text.lines().enumerate() {
                    let code = line.split('#').next().unwrap_or_default();
                    if !code.contains(FEATURE) {
                        continue;
                    }
                    if code.trim() == declaration {
                        declared += 1;
                    } else {
                        found.push(format!("{manifest}:{}: names {FEATURE}", n + 1));
                    }
                }
                if declared != 1 {
                    found.push(format!("{manifest}: declares {FEATURE} {declared} times"));
                }
            }
        }
    }
    // Every other crate src-tauri can be built on: one naming the feature would turn the client
    // core's switch on as a dependency of it.
    let mut crates: Vec<String> = std::fs::read_dir(root.join("crates"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| format!("crates/{}/Cargo.toml", entry.file_name().to_string_lossy()))
        .filter(|manifest| manifest != CORE)
        .collect();
    crates.sort();
    for manifest in crates {
        let Ok(text) = std::fs::read_to_string(root.join(&manifest)) else {
            continue;
        };
        read.push(manifest.clone());
        for (n, line) in text.lines().enumerate() {
            if line.split('#').next().unwrap_or_default().contains(FEATURE) {
                found.push(format!("{manifest}:{}: names {FEATURE}", n + 1));
            }
        }
    }
    for file in tauri_configs(root)
        .into_iter()
        .chain(BUILD_FILES.iter().map(|file| file.to_string()))
    {
        let Ok(text) = std::fs::read_to_string(root.join(&file)) else {
            continue;
        };
        read.push(file.clone());
        for (n, line) in text.lines().enumerate() {
            if line.contains(FEATURE) {
                found.push(format!("{file}:{}: names {FEATURE}", n + 1));
            }
        }
    }
    found
}

/// A scratch copy of every file in `files` from this tree, under a root of its own.
fn scratch_copy(label: &str, files: &[String]) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nexus-shipped-{label}-{nanos}"));
    for file in files {
        let to = root.join(file);
        std::fs::create_dir_all(to.parent().expect("a file has a folder")).unwrap();
        std::fs::copy(repository().join(file), &to).unwrap();
    }
    root
}

/// ★ The release never turns the road switch on: over this tree, nothing would, and the guard read
/// what the release builds from (both workflows, the three scripts that build Nexus, followed from
/// them, the manifests, the client core's and every other crate's among them, the Tauri config and
/// the build script). CONTROL: in scratch copies of those files, each way of turning the switch on
/// is named, at its file and line, and nothing else is: the macOS build passing the feature; the
/// Windows build given the test build's option; a build asking for every feature; the
/// cross-build's own default carrying it; another feature turning it on; a platform's Tauri
/// config; a cfg set by the build script; a new script the release runs, found by following the
/// workflow to it; the client core's own default carrying its switch; src-tauri's dependency on
/// the client core asking for it; and another crate asking for it. The copy untouched gives
/// nothing.
#[test]
fn the_release_never_turns_the_road_switch_on() {
    let mut read = Vec::new();
    assert_eq!(
        refusals(&repository(), &mut read),
        Vec::<String>::new(),
        "read: {read:?}"
    );
    for file in WORKFLOWS.into_iter().chain([
        "scripts/build-linux.sh",
        "scripts/Dockerfile.pi",
        CROSS,
        MANIFEST,
        CORE,
        "crates/tempo-app/Cargo.toml",
        "src-tauri/tauri.conf.json",
        "src-tauri/build.rs",
    ]) {
        assert!(
            read.iter().any(|kept| kept == file),
            "the guard never read {file}: {read:?}"
        );
    }

    let untouched = scratch_copy("untouched", &read);
    let mut again = Vec::new();
    assert_eq!(refusals(&untouched, &mut again), Vec::<String>::new());
    assert_eq!(again, read, "the copy is read as the tree is");
    std::fs::remove_dir_all(&untouched).unwrap();

    const RELEASE: &str = ".github/workflows/release.yml";
    let planted: [(&str, &str, &str, &str); 11] = [
        (
            RELEASE,
            "--features radio,custom-protocol",
            "--features radio,custom-protocol,bench-road-switch",
            "names bench-road-switch",
        ),
        (
            RELEASE,
            "bash scripts/build-windows-cross.sh\n",
            "bash scripts/build-windows-cross.sh --bench-road-switch\n",
            "names bench-road-switch",
        ),
        (
            RELEASE,
            "--features radio,custom-protocol",
            "--all-features",
            "asks for every feature",
        ),
        (
            CROSS,
            "FEATURES=radio,custom-protocol",
            "FEATURES=radio,custom-protocol,bench-road-switch",
            "names bench-road-switch",
        ),
        (
            MANIFEST,
            "\"tempo-audio/ai-cw\"]",
            "\"tempo-audio/ai-cw\", \"bench-road-switch\"]",
            "names bench-road-switch",
        ),
        (
            "src-tauri/tauri.windows.conf.json",
            "",
            "{ \"build\": { \"features\": [\"bench-road-switch\"] } }\n",
            "names bench-road-switch",
        ),
        (
            "src-tauri/build.rs",
            "fn main() {",
            "fn main() {\n    println!(\"cargo:rustc-cfg=feature=\\\"bench-road-switch\\\"\");",
            "names bench-road-switch",
        ),
        (
            "scripts/build-linux.sh",
            "set -euo pipefail\n",
            "set -euo pipefail\nbash scripts/build-bench.sh\n",
            "names bench-road-switch",
        ),
        (
            CORE,
            "bench-road-switch = []\n",
            "bench-road-switch = []\ndefault = [\"bench-road-switch\"]\n",
            "names bench-road-switch",
        ),
        (
            MANIFEST,
            "features = [\"device\"] }\n",
            "features = [\"device\", \"bench-road-switch\"] }\n",
            "names bench-road-switch",
        ),
        (
            "crates/tempo-app/Cargo.toml",
            "[dependencies]\n",
            "[dependencies]\ntempo-client = { path = \"../tempo-client\", features = [\"bench-road-switch\"] }\n",
            "names bench-road-switch",
        ),
    ];
    for (n, (file, from, to, says)) in planted.into_iter().enumerate() {
        let root = scratch_copy(&format!("planted-{n}"), &read);
        let path = root.join(file);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            text.contains(from),
            "{file} no longer has {from:?} to plant in"
        );
        std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
        // The new script the planted line in build-linux.sh runs, which names the feature.
        let named = if to.contains("scripts/build-bench.sh") {
            std::fs::write(
                root.join("scripts/build-bench.sh"),
                "cargo tauri build --features bench-road-switch\n",
            )
            .unwrap();
            "scripts/build-bench.sh"
        } else {
            file
        };
        let mut planted_read = Vec::new();
        let found = refusals(&root, &mut planted_read);
        assert!(
            found.len() == 1
                && found[0].starts_with(&format!("{named}:"))
                && found[0].ends_with(says),
            "planted in {file} ({to:?}), the guard said {found:?}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}

/// ★ A build without the feature has none of the switch: the command has no class, so both ends
/// would refuse it; no handler of the app's registers it; client mode refuses it `unclassed`, with
/// nothing run here; and the banner's status names no switch, so the screen draws none. CONTROL: a
/// command this computer has runs here through the same screen, and the status says how the link
/// stands.
#[cfg(not(feature = "bench-road-switch"))]
#[test]
fn without_the_feature_there_is_no_switch() {
    use crate::client_mode::tests::{ask, scratch, screens};
    use crate::client_mode::{Client, ThisComputer};
    use crate::command_classes::{class_of, classes, registered};
    use serde_json::json;
    use std::sync::Arc;

    assert_eq!(class_of("bench_road_switch"), None);
    assert!(classes().all(|(cmd, _)| cmd != "bench_road_switch"));
    assert!(!registered().contains(&"bench_road_switch"));
    let dir = scratch("no-road-switch");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(
        "0123abcd-0123-4567-89ab-0123456789ab".to_string(),
        this,
    ));
    let screens = screens(&client);
    assert_eq!(
        ask(
            &screens.main,
            "bench_road_switch",
            json!({ "to": "blockLink" })
        ),
        Err(json!("nexusClient:unclassed"))
    );
    assert!(screens.ran_here().is_empty(), "{:?}", screens.ran_here());
    assert_eq!(
        ask(&screens.main, "app_version", json!({})),
        Ok(json!("ran here")),
        "CONTROL"
    );
    let status = serde_json::to_value(client.status()).unwrap();
    let mut keys: Vec<&str> = status
        .as_object()
        .expect("the status is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "away",
            "computerBuild",
            "internet",
            "over",
            "phase",
            "pttRefusal",
            "reason",
            "signInBy",
            "signingAgain",
            "station",
            "stationBuild",
            "stillThere",
            "txHold",
            "udpBlocked"
        ]
    );
    assert_eq!(status["phase"], "connecting", "CONTROL");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ With the feature, the switch is a command of this computer's own: classed to run here (a row
/// in the build's contract too, so a test build is another build to a shipped one), routed here by
/// client mode and answered with where it is now, one road at a time, as the banner's status then
/// says; refused for a position it has not, and on a computer running its own station.
/// CONTROL: the status follows each move it takes, and none it refuses.
#[cfg(feature = "bench-road-switch")]
#[test]
fn with_the_feature_the_switch_runs_here_one_road_at_a_time() {
    use crate::bench_road_switch::{with_its_command, Position};
    use crate::client_mode::tests::{ask, scratch};
    use crate::client_mode::{way, Client, ThisComputer, Way};
    use crate::command_classes::{class_of, classes, Class};
    use serde_json::json;
    use std::sync::Arc;
    use tauri::ipc::Invoke;
    use tauri::test::MockRuntime;

    assert_eq!(class_of("bench_road_switch"), Some(Class::RunsHere));
    assert!(classes().any(|row| row == ("bench_road_switch", Class::RunsHere)));
    assert!(crate::command_dispatch::contract_text().contains("class bench_road_switch RunsHere\n"));
    assert_eq!(
        way("bench_road_switch", &json!({ "to": "blockLink" })),
        Way::Here
    );

    let dir = scratch("road-switch");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(
        "0123abcd-0123-4567-89ab-0123456789ab".to_string(),
        this,
    ));
    let local = with_its_command(Box::new(|invoke: Invoke<MockRuntime>| {
        invoke.resolver.resolve("ran here");
        true
    }));
    let routed = Arc::clone(&client);
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&client))
        .invoke_handler(move |invoke| routed.handle(invoke, &*local))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("a mock app");
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main window");
    let move_to = |to: &str| ask(&main, "bench_road_switch", json!({ "to": to }));
    assert_eq!(client.status().bench, Position::Normal);
    for (to, answer, now) in [
        ("blockLink", Ok(json!("blockLink")), Position::BlockLink),
        (
            "blockStopRoad",
            Err(json!("oneRoadAtATime")),
            Position::BlockLink,
        ),
        ("normal", Ok(json!("normal")), Position::Normal),
        (
            "blockStopRoad",
            Ok(json!("blockStopRoad")),
            Position::BlockStopRoad,
        ),
        (
            "blockLink",
            Err(json!("oneRoadAtATime")),
            Position::BlockStopRoad,
        ),
        (
            "blockStopRoad",
            Ok(json!("blockStopRoad")),
            Position::BlockStopRoad,
        ),
    ] {
        assert_eq!(move_to(to), answer, "to {to}");
        assert_eq!(client.status().bench, now, "after {to}");
    }
    assert!(move_to("both").is_err(), "a position the switch has not");
    assert_eq!(client.status().bench, Position::BlockStopRoad);
    assert_eq!(
        serde_json::to_value(client.status()).unwrap()["bench"],
        "blockStopRoad"
    );

    // A computer running its own station drives no roads, so there is nothing to move.
    let station = tauri::test::mock_builder()
        .invoke_handler(with_its_command(Box::new(|_: Invoke<MockRuntime>| false)))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("a mock app");
    let its_main = tauri::WebviewWindowBuilder::new(&station, "main", Default::default())
        .build()
        .expect("the main window");
    assert_eq!(
        ask(&its_main, "bench_road_switch", json!({ "to": "blockLink" })),
        Err(json!("notClient"))
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
