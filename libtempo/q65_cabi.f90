! Nexus: C ABI wrapper for native Q65 decode, built on the vendored WSJT-X GPL
! modem sources (lib/qra/q65 + q65_decode.f90) and Nico Palermo's (IV3NWV)
! qracodes C layer. Modelled on fst4_cabi.f90: q65_decode::decode is a clean
! self-contained OO decoder driven through a callback, with no nzhsym streaming
! ladder, no a7 cross-cycle table and no shared memory, so it is driven directly
! via a collector callback.
!
! TRANSMIT AND RECEIVE. q65_encode / q65_gen_wave are below, beside the decoder.
! Q65 shipped decode-only first; the encoder was cheap to add because genq65 and
! the qracodes C layer were ALREADY compiled into libtempo — q65_decode calls
! genq65 to regenerate a candidate's tone sequence on the RECEIVE path, the same
! shape as genfst4, so the encode chain was linked in all along.
!
! Encode and decode therefore share one symbol generator and cannot drift apart.
!
! Underlying Fortran/C:
!   q65_decode (q65_decode.f90) - OO decoder: ana64 -> q65_dec0 -> q65_loops
!                                 -> q65_dec1/q65_dec2 -> q65_subs.c -> q65.c
!                                 (Q-ary RA LDPC over GF(64))
!
! ⭐ ALL 5 T/R PERIODS x ALL 5 SUBMODES.
!   ntrperiod and nsubmode are ARGUMENTS, not parameters. The vendored Fortran was
!   always fully parametric — npts/nfft1/nfft2 derive from ntrperiod at
!   q65_decode.f90:137-139, the bandwidth table branches on it at :177-183, and
!   q65_dec0's iwave is an adjustable-size dummy `integer*2
!   iwave(0:12000*ntrperiod-1)` (q65.f90:62). Submode is likewise a runtime
!   variable (mode_q65: LL=64*(2+mode_q65) at q65.f90:84). The single-period pin
!   was in THIS file, not the modem.
!
!   That matters because Q65-30A is not the mode's main use. EME on VHF/UHF runs
!   Q65-60A/B/C; 6 m meteor/ionoscatter is where 30 belongs; 15 is troposcatter.
!   Pinning 30A shipped the mode's narrowest slice.
!
!   THE FRAME LENGTH IS THEREFORE A FUNCTION OF THE PERIOD: npts = ntrperiod*12000,
!   from 180000 (15 s) to 3600000 (300 s). The caller MUST supply npts samples for
!   the period it asks for; Q65_NMAX_MAX below is the ceiling, not the contract.
!
!   ⚠️ The state manifest flags anything cached on period or submode as
!   chain-specific the moment two chains differ. That was a latent note while only
!   one combination was built; now that all 25 are reachable it is live, and any
!   future per-chain swap must treat the Q65 caches accordingly.
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

  ! Ceiling only — the ACTUAL frame length is ntrperiod*12000, chosen per call.
  integer, parameter :: Q65_NMAX_MAX = 300 * 12000         ! 3,600,000 samples (300 s)
  integer, parameter :: Q65_MAXDEC   = 100                 ! matches decodes(100) in q65_decode

  ! The periods upstream supports. Anything else is rejected rather than clamped:
  ! a wrong period reads the wrong span of the caller's buffer, and silently
  ! decoding the wrong window is worse than refusing.
  integer, parameter :: Q65_NPERIODS = 5
  integer, parameter :: Q65_PERIODS(Q65_NPERIODS) = [15, 30, 60, 120, 300]

  ! Q65 channel symbols per transmission: 63 data + 22 sync (the isync table in
  ! genq65). Upstream's NUM_Q65_SYMBOLS.
  integer, parameter :: Q65_NN = 85

  ! Symbol length in samples at 12 kHz, indexed like Q65_PERIODS. Verbatim from
  ! WSJT-X's own table (lib/q65params.f90 `data nsps/.../`, and repeated at
  ! mainwindow.cpp:12714) - NOT derived, because 120 s is 16000 rather than the
  ! 16384 a power-of-two guess would give, and 300 s is 41472.
  integer, parameter :: Q65_NSPS(Q65_NPERIODS) = [1800, 3600, 7200, 16000, 41472]

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
  ! q65_decode_frame : decode EVERY Q65 signal in one T/R period.
  !
  !   iwave         : ntrperiod*12000 int16 audio samples @ 12 kHz. The caller
  !                   sizes this from the period it asks for — 180000 at 15 s,
  !                   3600000 at 300 s. Supplying fewer reads past the end.
  !   ntrperiod     : 15, 30, 60, 120 or 300 (seconds). Anything else => -1.
  !   nsubmode      : 0..4 for A..E (tone spacing). Anything else => -1.
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
  function q65_decode_frame(iwave, ntrperiod, nsubmode, nfa, nfb, ndepth, &
       mycall, hiscall, hisgrid, nqso_progress, nfqso_in, out, max_out) &
       result(ndec) bind(C, name="q65_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(*)
    integer(c_int), value,  intent(in)  :: ntrperiod, nsubmode
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress
    integer(c_int), value,  intent(in)  :: nfqso_in, max_out
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    character(kind=c_char), intent(in)  :: hisgrid(*)
    type(q65_decode_t),     intent(out) :: out(*)
    integer(c_int)                      :: ndec

    type(q65_decoder)  :: decoder
    ! ALLOCATABLE, not automatic. At 300 s the frame is 3,600,000 samples = 7.2 MB,
    ! which blows the default 8 MB stack once locals and the decoder's own frames
    ! are on it. (Stock q65sim segfaults for exactly this reason under the default
    ! rlimit.) Allocating npts exactly also avoids paying the 300 s cost on a 15 s
    ! decode, which is 20x smaller.
    integer(kind=2), allocatable :: iwave_l(:)
    character(len=12)  :: mycall_f, hiscall_f
    character(len=6)   :: hisgrid_f
    integer            :: nfqso, ndepth_l, i, j, n, ncopy, npts
    integer            :: nutc, nqd, ntol, max_drift, ncontest, navg0
    integer            :: nqf(20)
    real               :: emedelay
    logical            :: lclearave, single_decode, lagain, lnewdat, lapcqonly

    ndec = 0
    if (max_out <= 0) return

    ! REJECT rather than clamp. An unsupported period would make the modem read a
    ! different span of iwave than the caller sized, and a decode off the wrong
    ! window is a plausible-looking wrong answer, not an obvious failure.
    if (.not. any(Q65_PERIODS == ntrperiod)) then
       ndec = -1
       return
    end if
    if (nsubmode < 0 .or. nsubmode > 4) then
       ndec = -1
       return
    end if
    npts = ntrperiod * 12000

    gq_count = 0
    allocate(iwave_l(npts))
    iwave_l(1:npts) = int(iwave(1:npts), kind=2)
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
    call decoder%decode(q65_collect_cb, iwave_l, nqd, nutc, ntrperiod,       &
         nsubmode, nfqso, ntol, ndepth_l, nfa, nfb, lclearave,               &
         single_decode, lagain, max_drift, lnewdat, emedelay, mycall_f,      &
         hiscall_f, hisgrid_f, nqso_progress, ncontest, lapcqonly, navg0, nqf)
    deallocate(iwave_l)

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
  ! q65_encode_msg : message text -> the 85 Q65 channel symbols.
  !
  ! NOT `q65_encode` — that name is TAKEN by upstream's own qracodes API
  ! (`int q65_encode(const q65_codec_ds*, int*, const int*)`, q65.h:65), which
  ! encodes a codeword rather than a message and is already linked into libtempo.
  ! Using it here linked cleanly right up to a duplicate-symbol error.
  !
  !   msg       : message text (C string, <= 37 chars)
  !   msg_len   : length of msg
  !   itone_out : caller buffer, Q65_NN entries
  !   nsym_out  : out = Q65_NN on success, -1 if the message will not pack
  !
  ! Straight into the vendored genq65 — the SAME routine the decoder already
  ! calls to regenerate a candidate's tone sequence, so encode and decode cannot
  ! drift apart. It emits itone(1:85): 22 sync symbols at tone 0 (the isync
  ! table) interleaved with 63 data symbols offset by +1, because Q65 transmits
  ! data symbol 0 on tone 1.
  !
  ! ⭐ SUBMODE AND PERIOD DO NOT APPEAR HERE, deliberately. Q65's submode scales
  ! only the TONE SPACING and the period only the symbol DURATION; neither
  ! changes the symbol values. Both enter in q65_gen_wave below. Upstream splits
  ! it the same way (genq65 takes neither).
  !-------------------------------------------------------------------------
  function q65_encode_msg(msg, msg_len, itone_out) result(nsym_out) &
       bind(C, name="q65_encode_msg")
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len
    integer(c_int),         intent(out) :: itone_out(Q65_NN)
    integer(c_int) :: nsym_out

    character(len=37) :: msg37, msgsent37
    integer :: itone(Q65_NN)
    integer :: i, n, ichk, i3, n3

    msg37 = ' '
    n = min(msg_len, 37)
    do i = 1, n
       if (msg(i) == c_null_char) exit
       msg37(i:i) = msg(i)
    end do

    ichk = 0
    i3 = -1
    n3 = -1
    itone = 0
    call genq65(msg37, ichk, msgsent37, itone, i3, n3)
    ! genq65 leaves i3/n3 at -1 when pack77 could not place the message. Refuse
    ! rather than transmit whatever happened to be in itone.
    if (i3 < 0 .and. n3 < 0) then
       nsym_out = -1
       return
    end if
    itone_out(1:Q65_NN) = itone(1:Q65_NN)
    nsym_out = Q65_NN
  end function q65_encode_msg

  !-------------------------------------------------------------------------
  ! q65_gen_wave : channel symbols -> real audio, at 12 kHz.
  !
  !   itone     : the 85 symbols from q65_encode_msg
  !   nsym      : symbol count (Q65_NN)
  !   ntrperiod : T/R period, seconds - sets the symbol duration
  !   nsubmode  : 0..4 for A..E - sets the tone spacing
  !   fsample   : output sample rate (Hz)
  !   f0        : audio carrier (Hz)
  !   wave_out  : caller buffer (capacity nwave_out)
  !   nwave_out : in = capacity; out = samples produced, or -1 on refusal
  !
  ! ⭐ THE TWO SCALING RULES, from WSJT-X 3.0.2 and cross-checked against two
  ! independent places in it:
  !
  !     nsps    = Q65_NSPS(period)          symbol length in samples at 12 kHz
  !     baud    = 12000 / nsps              keying rate
  !     spacing = baud * 2**nsubmode        A=1x, B=2x, C=4x, D=8x, E=16x
  !
  ! The submode factor is REAL and easy to miss: mainwindow.cpp:8038 builds a
  ! 48 kHz PREVIEW buffer with `toneSpacing=fsample/nsps4`, which is submode A
  ! regardless of the selected submode. That is NOT the on-air path. The
  ! transmitted signal comes from MainWindow::transmit at mainwindow.cpp:12721:
  !
  !     int mode65=pow(2.0,double(m_nSubMode));
  !     toneSpacing=mode65*12000.0/nsps;
  !
  ! which agrees with q65sim.f90:176 (`freq = f0 + itone(isym)*baud*mode65`) and
  ! with lib/q65params.f90 (`spacing=baud*2**(j-1)`). Getting this wrong emits a
  ! signal at submode-A spacing that no correspondent can decode.
  !
  ! Modulation is plain continuous-phase MFSK — phase accumulates ACROSS symbol
  ! boundaries and is never reset, exactly as upstream's genwave.f90 and q65sim
  ! do it. Resetting per symbol would splatter the spectrum.
  !-------------------------------------------------------------------------
  function q65_gen_wave(itone, nsym, ntrperiod, nsubmode, fsample, f0, &
       wave_out, nwave_cap) result(nwave_out) bind(C, name="q65_gen_wave")
    integer(c_int),        intent(in)    :: itone(*)
    integer(c_int), value, intent(in)    :: nsym, ntrperiod, nsubmode
    real(c_float),  value, intent(in)    :: fsample, f0
    real(c_float),         intent(inout) :: wave_out(*)
    integer(c_int), value, intent(in)    :: nwave_cap
    integer(c_int) :: nwave_out

    integer  :: nsps, nwave, i, j, k, ip
    real*8   :: dt, phi, dphi, twopi, freq, baud, tonespacing

    nwave_out = -1
    if (nsym /= Q65_NN) return
    if (nsubmode < 0 .or. nsubmode > 4) return
    ip = q65_period_index(ntrperiod)
    if (ip < 1) return

    nsps  = Q65_NSPS(ip)
    nwave = nsym * nsps
    if (nwave_cap < nwave) return

    ! Tone spacing scales with the submode; the keying rate does not.
    baud        = 12000.d0 / dble(nsps)
    tonespacing = baud * (2.d0 ** nsubmode)

    dt    = 1.d0 / dble(fsample)
    twopi = 8.d0 * atan(1.d0)
    phi   = 0.d0
    k     = 0
    do j = 1, nsym
       freq = dble(f0) + dble(itone(j)) * tonespacing
       dphi = twopi * freq * dt
       do i = 1, nsps
          k = k + 1
          wave_out(k) = real(sin(phi))
          phi = phi + dphi
          if (phi > twopi) phi = phi - twopi
       end do
    end do
    nwave_out = nwave
  end function q65_gen_wave

  !-------------------------------------------------------------------------
  ! q65_period_index : 1..Q65_NPERIODS for a supported T/R period, else -1.
  ! Reject, never clamp — the same rule the decode side follows.
  !-------------------------------------------------------------------------
  integer function q65_period_index(ntrperiod)
    integer, intent(in) :: ntrperiod
    integer :: i
    q65_period_index = -1
    do i = 1, Q65_NPERIODS
       if (Q65_PERIODS(i) == ntrperiod) then
          q65_period_index = i
          return
       end if
    end do
  end function q65_period_index

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
