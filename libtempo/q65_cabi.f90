! Nexus: C ABI wrapper for native Q65 decode, built on the vendored WSJT-X GPL
! modem sources (lib/qra/q65 + q65_decode.f90) and Nico Palermo's (IV3NWV)
! qracodes C layer. Modelled on fst4_cabi.f90: q65_decode::decode is a clean
! self-contained OO decoder driven through a callback, with no nzhsym streaming
! ladder, no a7 cross-cycle table and no shared memory, so it is driven directly
! via a collector callback.
!
! RX-ONLY, DELIBERATELY. There is no q65_encode / gen_q65wave here. Q65 is being
! added as a decode-only mode: `Capabilities { tx: false }` on the Rust side means
! modes::tx_mode() refuses to hand it to the transmit path. Adding TX means adding
! those entry points here AND flipping that flag AND passing the FT-mode TX hard
! gate — three deliberate steps, not an oversight.
!
! (genq65 IS compiled into libtempo regardless: q65_decode calls it to regenerate a
! candidate's tone sequence on the RECEIVE path, the same shape as genfst4.)
!
! Underlying Fortran/C:
!   q65_decode (q65_decode.f90) - OO decoder: ana64 -> q65_dec0 -> q65_loops
!                                 -> q65_dec1/q65_dec2 -> q65_subs.c -> q65.c
!                                 (Q-ary RA LDPC over GF(64))
!
! ⭐ Q65-30A ONLY — ONE PERIOD, ONE SUBMODE.
!   Upstream offers 5 T/R periods x 5 submodes (A-E). q65_decode sizes its frame
!   from ntrperiod (npts = ntrperiod*12000, q65_decode.f90:109), so the buffer
!   contract depends on it. This wrapper pins ntrperiod=30 / nsubmode=0, giving
!   Q65_NMAX = 360000 samples. Exposing more needs a per-period entry point plus
!   Rust-side work to express a selectable period — and note that the state
!   manifest flags anything cached on period or submode as chain-specific the
!   moment two chains differ, even though only one combination is built today.
!
! ⭐ EVERY CALL IS INDEPENDENT — lclearave is pinned .true.
!   q65 supports multi-period message averaging, accumulating symbol-spectrum
!   power in s1a across calls (q65.f90:303, the exponentially weighted update).
!   The slice index comes from UTC alone (q65_decode.f90:161-167), so with the
!   fixed nutc=0 below every call would land on slice 0 and frame N would be
!   decoded partly from frames 1..N-1. That is precisely the cross-frame
!   contamination that cost a calibration round on the FT8 side (batched jt9
!   replay; see reference-decode-parity-lab). Pinning lclearave=.true. calls
!   q65_clravg (q65_decode.f90:168) at the top of every decode, so each frame
!   stands alone. Real multi-period averaging needs a stateful session API plus
!   the per-chain swap the manifest specifies, not a flag flip here.

module q65_cabi
  use iso_c_binding
  use q65_decode, only: q65_decoder
  implicit none

  integer, parameter :: Q65_NTRPERIOD = 30                 ! seconds; see banner
  integer, parameter :: Q65_NSUBMODE  = 0                  ! 0 = submode A
  integer, parameter :: Q65_NMAX      = 30 * 12000         ! 360000 samples @ 12 kHz
  integer, parameter :: Q65_MAXDEC    = 100                ! matches decodes(100) in q65_decode

  ! Interop result struct. Layout MUST match q65_decode_t in libtempo.h, and is
  ! byte-compatible with ft8/ft4/fst4_decode_t (64 bytes, 4-byte aligned, 2-byte
  ! pad after message) so the Rust side can reuse the same shape. Only the last
  ! two fields differ in MEANING, and they are named for what Q65 actually
  ! reports rather than reusing FT8's nap/qual.
  type, bind(C) :: q65_decode_t
     real(c_float)          :: sync        ! snr1: sync-curve correlation metric
     integer(c_int)         :: snr         ! nsnr: SNR estimate, dB in 2500 Hz
     real(c_float)          :: dt
     real(c_float)          :: freq
     character(kind=c_char) :: message(38)
     integer(c_int)         :: idec        ! decode type: 0=q0, 1=q1, 2=q2, 3=q3 list
     integer(c_int)         :: nused       ! T/R periods averaged (always 1 here)
  end type q65_decode_t

  ! Module-level results buffer populated by the collector callback.
  ! NOT thread-safe: q65_decode_frame() must not be called concurrently.
  type :: q65_result_rec
     real    :: sync
     integer :: snr
     real    :: dt
     real    :: freq
     character(len=37) :: message
     integer :: idec
     integer :: nused
  end type q65_result_rec

  type(q65_result_rec), save :: gq_results(Q65_MAXDEC)
  integer,              save :: gq_count = 0

contains

  !-------------------------------------------------------------------------
  ! q65_collect_cb : collector matching the q65_decode_callback interface
  !                  (q65_decode.f90:14-28).
  !
  ! Q65's callback carries idec and nused where FT8's carries nap and qual.
  ! `nutc` and `ntrperiod` are echoed back from what we passed in and are
  ! dropped: the caller already knows both, and nutc is only a slot label for
  ! the diagnostic dump the headless build excised.
  !-------------------------------------------------------------------------
  subroutine q65_collect_cb(this, nutc, snr1, nsnr, dt, freq, decoded, &
       idec, nused, ntrperiod)
    class(q65_decoder), intent(inout) :: this
    integer,           intent(in) :: nutc
    real,              intent(in) :: snr1
    integer,           intent(in) :: nsnr
    real,              intent(in) :: dt
    real,              intent(in) :: freq
    character(len=37), intent(in) :: decoded
    integer,           intent(in) :: idec
    integer,           intent(in) :: nused
    integer,           intent(in) :: ntrperiod

    if (gq_count >= Q65_MAXDEC) return
    gq_count = gq_count + 1
    gq_results(gq_count)%sync    = snr1
    gq_results(gq_count)%snr     = nsnr
    gq_results(gq_count)%dt      = dt
    gq_results(gq_count)%freq    = freq
    gq_results(gq_count)%message = decoded
    gq_results(gq_count)%idec    = idec
    gq_results(gq_count)%nused   = nused
  end subroutine q65_collect_cb

  !-------------------------------------------------------------------------
  ! q65_decode_frame : decode EVERY Q65 signal in a 360000-sample frame.
  !
  !   iwave         : Q65_NMAX (360000) int16 audio samples @ 12 kHz (30 s)
  !   nfa, nfb      : frequency search band edges (Hz)
  !   ndepth        : 1..3 (3 = deepest; <=0 defaults to 3)
  !   mycall,hiscall: NUL/space-terminated callsigns for AP (may be empty)
  !   hisgrid       : NUL/space-terminated 6-char grid for AP (may be empty)
  !   nqso_progress : QSO progress index (AP pass schedule)
  !   nfqso_in      : QSO/RX audio freq (Hz) being worked. Pass 0 / out-of-band
  !                   => band centre.
  !   out           : caller array of q65_decode_t (capacity max_out)
  !   max_out       : capacity of out
  !
  !   Returns the number of decodes found (>= 0), or -1 on error.
  !
  ! NOT thread-safe (the modem keeps process-global SAVE state + FFTW plans, and
  ! Q65 adds C statics on top: `codec` in q65_subs.c and the q65_hist ring). The
  ! per-chain subset is classified in modem-state-manifest.toml GROUP H: 238
  ! symbols, ~12.3 MB of class-1 state.
  !-------------------------------------------------------------------------
  function q65_decode_frame(iwave, nfa, nfb, ndepth, mycall, hiscall, hisgrid, &
       nqso_progress, nfqso_in, out, max_out) result(ndec) &
       bind(C, name="q65_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(Q65_NMAX)
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress
    integer(c_int), value,  intent(in)  :: nfqso_in, max_out
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    character(kind=c_char), intent(in)  :: hisgrid(*)
    type(q65_decode_t),     intent(out) :: out(*)
    integer(c_int)                      :: ndec

    type(q65_decoder)  :: decoder
    integer(kind=2)    :: iwave_l(Q65_NMAX)
    character(len=12)  :: mycall_f, hiscall_f
    character(len=6)   :: hisgrid_f
    integer            :: nfqso, ndepth_l, i, j, n, ncopy
    integer            :: nutc, nqd, ntol, max_drift, ncontest, navg0
    integer            :: nqf(20)
    real               :: emedelay
    logical            :: lclearave, single_decode, lagain, lnewdat, lapcqonly

    ndec = 0
    if (max_out <= 0) return

    gq_count = 0
    iwave_l(1:Q65_NMAX) = int(iwave(1:Q65_NMAX), kind=2)
    call c_to_fstr_q65(mycall,  mycall_f, 12)
    call c_to_fstr_q65(hiscall, hiscall_f, 12)
    call c_to_fstr_q65(hisgrid, hisgrid_f, 6)

    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3
    ! Centre the deep AP passes on the operator's QSO/RX freq when supplied,
    ! else band mid — same rule as ft4_cabi / fst4_cabi.
    if (nfqso_in >= nfa .and. nfqso_in <= nfb) then
       nfqso = nfqso_in
    else
       nfqso = (nfa + nfb) / 2
    end if

    ! Fixed arguments. Where a value differs from upstream's GUI-driven default
    ! the reason is given, because each one is a behavioural choice:
    !
    !   nqd=0        Full-band search across nfa..nfb. nqd=1 is upstream's
    !                "quick decode" pass, which builds the full-AP candidate
    !                list around nfqso only (q65_decode.f90:214-222); upstream
    !                calls the decoder twice, once each way. A band scanner
    !                wants the wide pass, and AP still applies via ft8apset
    !                (:228), which is unconditional.
    !   nutc=0       Only a slot label for the excised diagnostic dump, plus the
    !                iseq averaging-slice selector — and averaging is disabled
    !                below, so the slice never matters.
    !   lclearave    .true. — see the banner. Every frame stands alone.
    !   lnewdat      .true. — the frame we just handed over IS new data; this
    !                gates the s1a spectrum update at q65.f90:299.
    !   ncontest=0   No contest mode. The contest path needs the caller history
    !                that used to come from tsil.3q, whose file read the headless
    !                surgery removed (q65_decode.f90:119-158) precisely because
    !                it let one chain's callers reach another through the disk.
    !   single_decode .false. — we want every signal in the passband, not just
    !                the one at nfqso.
    !   lagain       .false. — this is upstream's manual "decode again" flag; it
    !                forces Deep depth and consults the q65_hist ring, which is
    !                the cross-chain AP hazard the manifest flags.
    !   max_drift=0  No frequency-drift search. Upstream exposes this as a GUI
    !                setting; 0 is its off position.
    !   lapcqonly    .false. — do not restrict AP to CQ calls only.
    nutc          = 0
    nqd           = 0
    ntol          = 20
    max_drift     = 0
    ncontest      = 0
    emedelay      = 0.0
    lclearave     = .true.
    lnewdat       = .true.
    single_decode = .false.
    lagain        = .false.
    lapcqonly     = .false.
    navg0         = 0
    nqf           = 0

    decoder%callback => q65_collect_cb
    call decoder%decode(q65_collect_cb, iwave_l, nqd, nutc, Q65_NTRPERIOD,   &
         Q65_NSUBMODE, nfqso, ntol, ndepth_l, nfa, nfb, lclearave,           &
         single_decode, lagain, max_drift, lnewdat, emedelay, mycall_f,      &
         hiscall_f, hisgrid_f, nqso_progress, ncontest, lapcqonly, navg0, nqf)

    ! ncopy == max_out means the cap was hit and decodes were dropped: raise
    ! Q65_MAXDEC and the Rust-side MAX_DECODES together.
    ncopy = min(gq_count, max_out)
    do i = 1, ncopy
       out(i)%sync  = gq_results(i)%sync
       out(i)%snr   = gq_results(i)%snr
       out(i)%dt    = gq_results(i)%dt
       out(i)%freq  = gq_results(i)%freq
       out(i)%idec  = gq_results(i)%idec
       out(i)%nused = gq_results(i)%nused
       do j = 1, 38
          out(i)%message(j) = c_null_char
       end do
       n = len_trim(gq_results(i)%message)
       if (n > 37) n = 37
       do j = 1, n
          out(i)%message(j) = gq_results(i)%message(j:j)
       end do
    end do

    ndec = gq_count
  end function q65_decode_frame

  !-------------------------------------------------------------------------
  ! c_to_fstr_q65 : marshal a NUL/space-terminated C string into a Fortran
  ! character of the requested length. Module-scoped and suffixed so it cannot
  ! collide with ft8/ft4/fst4_cabi's copies. Takes a length because Q65 needs
  ! both character(12) callsigns and a character(6) grid.
  !-------------------------------------------------------------------------
  subroutine c_to_fstr_q65(cstr, fstr, nlen)
    character(kind=c_char), intent(in)  :: cstr(*)
    character(len=*),       intent(out) :: fstr
    integer,                intent(in)  :: nlen
    integer :: i
    fstr = ' '
    do i = 1, min(nlen, len(fstr))
       if (cstr(i) == c_null_char) exit
       fstr(i:i) = cstr(i)
    end do
  end subroutine c_to_fstr_q65

end module q65_cabi
