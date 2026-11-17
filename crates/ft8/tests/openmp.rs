//! libtempo's OpenMP runtime, checked the way the app checks it at launch: through
//! [`ft8::openmp_selfcheck`], on the decoder thread that every OpenMP parallel region in the app
//! runs on. (On Windows a thread that has run a parallel region must never exit: see
//! `tempo_fast_sys::openmp_probe_on_this_thread`.)

/// The runtime links, starts a team of the size asked for, and every worker has the 4 MB of stack
/// the multithreaded FT8 decoder's arrays need (omp_env.c sets WSJT-X's OMP_STACKSIZE of 10M
/// before libgomp starts; without it a worker would crash here).
#[test]
fn openmp_runtime_runs_a_team_with_decoder_sized_stacks() {
    let check = ft8::openmp_selfcheck(4);
    assert_eq!(check.ran, 4, "{check:?}");
    assert!(check.max_threads >= 1, "{check:?}");
    // Twice, as a relaunch of the check would: the same long-lived master reuses its team.
    assert_eq!(ft8::openmp_selfcheck(4).ran, 4);
    if std::env::var_os("OMP_STACKSIZE").is_some_and(|v| v != "10M") {
        return; // an operator's own setting is left alone, by design
    }
    assert_eq!(
        std::env::var("OMP_STACKSIZE").as_deref(),
        Ok("10M"),
        "libtempo's constructor sets WSJT-X's worker stack size before libgomp reads it"
    );
}
