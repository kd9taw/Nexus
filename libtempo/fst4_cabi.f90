! Nexus: C ABI wrapper for native FST4 decode, built on the vendored WSJT-X GPL
! modem sources (lib/fst4 + fst4_decode.f90). Modelled on ft4_cabi.f90, not
! ft8_cabi.f90: like FT4, the WSJT-X FST4 decoder (fst4_decode::decode) is a clean
! self-contained OO decoder driven through a callback, with no nzhsym streaming
! ladder, no a7 cross-cycle table and no shared memory. So it is driven directly
! via a collector callback, exactly as ft4_cabi does.
!
! RX-ONLY, DELIBERATELY. There is no fst4_encode / fst4_gen_wave here. FST4 is
! being added as a decode-only mode: `Capabilities { tx: false }` on the Rust side
! means modes::tx_mode() refuses to hand it to the transmit path. Adding TX means
! adding genfst4 + a gen_fst4wave wrapper here AND flipping that flag AND passing
! the FT-mode TX hard gate — three deliberate steps, not an oversight.
!
! (gen_fst4wave IS compiled into libtempo regardless: fst4_decode calls it to
! regenerate and subtract a decoded signal, so it is on the RECEIVE path.)
!
! Underlying Fortran:
!   fst4_decode  (fst4_decode.f90)  - OO decoder: get_candidates_fst4 -> sync_fst4
!                                     -> fst4_downsample -> get_fst4_bitmetrics
!                                     -> decode240_101 / decode240_74 (+OSD)
!
! ⭐ ALL 7 T/R PERIODS, AND BOTH MODES (FST4 + FST4W).
!   fst4_decode.f90:204-240 sizes everything from ntrperiod:
!       ntrperiod   15    30    60    120    300     900     1800  (seconds)
!       nmax    180000 360000 720000 1440000 3600000 10800000 21600000  (samples)
!   ntrperiod and iwspr are ARGUMENTS. The caller supplies ntrperiod*12000 samples;
!   the routine reads nfft1, which the table keeps <= nmax at every period.
!
!   iwspr=0 is FST4 (QSO mode, 77-bit messages); iwspr=1 is FST4W, the WSPR-like
!   BEACON mode (50-bit messages, no AP decoding). FST4W is the reason the period
!   had to become an argument at all: its standard beacon intervals are 120/300/
!   900/1800 s, so a wrapper pinned to 15 s could not do FST4W in any useful form.
!
!   ⚠️ FST4W HASHED CALLSIGNS DO NOT RESOLVE. The k50 deep-decode path looks up
!   hashed calls in a table that upstream populates from fst4w_calls.txt, a
!   GUI-side file the headless surgery removed (fst4_decode.f90:125-132). With
!   nwcalls=0 the lookup finds nothing and reports the `<...>` hash form — exactly
!   what an empty file produced upstream. Beacon reception, SNR and grid all work;
!   only the resolution of a previously-heard hashed call is missing. Restoring it
!   means feeding the table through this ABI, not reopening a file.

module fst4_cabi
  use iso_c_binding
  use fst4_decode, only: fst4_decoder
  implicit none

  ! Ceiling only — the ACTUAL frame length is ntrperiod*12000, chosen per call.
  integer, parameter :: FST4_NMAX_MAX = 1800 * 12000       ! 21,600,000 samples (1800 s)
  integer, parameter :: FST4_MAXDEC   = 100                ! matches MAXCAND in fst4_decode

  ! The periods upstream supports. Anything else is rejected rather than clamped:
  ! a wrong period makes the modem read a different span than the caller sized.
  integer, parameter :: FST4_NPERIODS = 7
  integer, parameter :: FST4_PERIODS(FST4_NPERIODS) = [15, 30, 60, 120, 300, 900, 1800]

  ! FST4 channel symbols per transmission: 120 data + 40 sync (five 8x4 sync
  ! vectors). Upstream's NN in lib/fst4/fst4_params.f90; NUM_FST4_SYMBOLS in the Qt
  ! layer. FST4 is 4-FSK, so itone values are 0..3 - unlike Q65's 0..64.
  integer, parameter :: FST4_NN = 160

  ! Symbol length in samples at 12 kHz, indexed like FST4_PERIODS. Verbatim from
  ! upstream, where the SAME table appears in three places that must agree:
  ! fst4_decode.f90:206-236, and both TX blocks in mainwindow.cpp (:8016 and
  ! :12689). Not derived - 120 s is 8200 and 900 s is 66560.
  integer, parameter :: FST4_NSPS(FST4_NPERIODS) = &
       [720, 1680, 3888, 8200, 21504, 66560, 134400]

  ! Interop result struct. Layout MUST match fst4_decode_t in libtempo.h.
  ! Identical to ft4_decode_t (64 bytes) so the Rust side can share a shape.
  type, bind(C) :: fst4_decode_t
     real(c_float)          :: sync
     integer(c_int)         :: snr
     real(c_float)          :: dt
     real(c_float)          :: freq
     character(kind=c_char) :: message(38)
     integer(c_int)         :: nap
     real(c_float)          :: qual
  end type fst4_decode_t

  ! Module-level results buffer populated by the collector callback.
  ! NOT thread-safe: fst4_decode_frame() must not be called concurrently.
  type :: fst4_result_rec
     real    :: sync
     integer :: snr
     real    :: dt
     real    :: freq
     character(len=37) :: message
     integer :: nap
     real    :: qual
  end type fst4_result_rec

  type(fst4_result_rec), save :: gs4_results(FST4_MAXDEC)
  integer,               save :: gs4_count = 0

contains

  !-------------------------------------------------------------------------
  ! fst4_collect_cb : collector matching the fst4_decode_callback interface.
  !
  ! FST4's callback carries four arguments FT4's does not: nutc, ntrperiod, and
  ! fmid/w50 (the Doppler-spread pair). fmid/w50 are dropped here — upstream only
  ! ever populates them from dopspread, which was a plotting aid gated on a
  ! `plotspec` marker file and is excised in the headless build, so both now carry
  ! the -999.0 "not measured" sentinel. Nothing in Nexus consumes them.
  !-------------------------------------------------------------------------
  subroutine fst4_collect_cb(this, nutc, sync, nsnr, dt, freq, decoded, &
       nap, qual, ntrperiod, fmid, w50)
    class(fst4_decoder), intent(inout) :: this
    integer,           intent(in) :: nutc
    real,              intent(in) :: sync
    integer,           intent(in) :: nsnr
    real,              intent(in) :: dt
    real,              intent(in) :: freq
    character(len=37), intent(in) :: decoded
    integer,           intent(in) :: nap
    real,              intent(in) :: qual
    integer,           intent(in) :: ntrperiod
    real,              intent(in) :: fmid
    real,              intent(in) :: w50

    if (gs4_count >= FST4_MAXDEC) return
    gs4_count = gs4_count + 1
    gs4_results(gs4_count)%sync    = sync
    gs4_results(gs4_count)%snr     = nsnr
    gs4_results(gs4_count)%dt      = dt
    gs4_results(gs4_count)%freq    = freq
    gs4_results(gs4_count)%message = decoded
    gs4_results(gs4_count)%nap     = nap
    gs4_results(gs4_count)%qual    = qual
  end subroutine fst4_collect_cb

  !-------------------------------------------------------------------------
  ! fst4_decode_frame : decode EVERY FST4/FST4W signal in one T/R period.
  !
  !   iwave         : ntrperiod*12000 int16 audio samples @ 12 kHz
  !   ntrperiod     : 15|30|60|120|300|900|1800 (s). Anything else => -1.
  !   iwspr_in      : 0 = FST4 (QSO, 77-bit), 1 = FST4W (beacon, 50-bit). Else -1.
  !   nfa, nfb      : frequency search band edges (Hz)
  !   ndepth        : 1..3 (3 = full bp+osd; <=0 defaults to 3)
  !   mycall,hiscall: NUL/space-terminated callsigns for AP (may be empty)
  !   nqso_progress : QSO progress index (AP pass schedule)
  !   nfqso_in      : QSO/RX audio freq (Hz) being worked. Pass 0 / out-of-band
  !                   => band centre.
  !   out           : caller array of fst4_decode_t (capacity max_out)
  !   max_out       : capacity of out
  !
  !   Returns the number of decodes found (>= 0), or -1 on error.
  !
  ! NOT thread-safe (the modem keeps process-global SAVE state + FFTW plans). The
  ! per-chain subset is classified in modem-state-manifest.toml GROUP G: 33
  ! symbols, 106,060 bytes, almost all of it in fst4_decode.f90.
  !-------------------------------------------------------------------------
  function fst4_decode_frame(iwave, ntrperiod, iwspr_in, nfa, nfb, ndepth, &
       mycall, hiscall, nqso_progress, nfqso_in, out, max_out) result(ndec) &
       bind(C, name="fst4_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(*)
    integer(c_int), value,  intent(in)  :: ntrperiod, iwspr_in
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth, nqso_progress
    integer(c_int), value,  intent(in)  :: nfqso_in, max_out
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(fst4_decode_t),    intent(out) :: out(*)
    integer(c_int)                      :: ndec

    type(fst4_decoder) :: decoder
    ! ALLOCATABLE, not automatic: at 1800 s the frame is 21.6 M samples = 43 MB,
    ! which no default stack survives. Allocating npts exactly also avoids paying
    ! the 1800 s cost on a 15 s decode, which is 120x smaller.
    integer(kind=2), allocatable :: iwave_l(:)
    character(len=12)  :: mycall_f, hiscall_f
    integer            :: nfqso, ndepth_l, i, j, n, ncopy, npts
    integer            :: nutc, nexp_decode, ntol, iwspr
    real               :: emedelay
    logical            :: lagain, lapcqonly, lprinthash22

    ndec = 0
    if (max_out <= 0) return

    ! REJECT rather than clamp — see the banner.
    if (.not. any(FST4_PERIODS == ntrperiod)) then
       ndec = -1
       return
    end if
    if (iwspr_in /= 0 .and. iwspr_in /= 1) then
       ndec = -1
       return
    end if
    npts = ntrperiod * 12000

    gs4_count = 0
    allocate(iwave_l(npts))
    iwave_l(1:npts) = int(iwave(1:npts), kind=2)
    call c_to_fstr12_fst4(mycall,  mycall_f)
    call c_to_fstr12_fst4(hiscall, hiscall_f)

    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3
    ! Centre the deep AP passes on the operator's QSO/RX freq when supplied,
    ! else band mid — same rule as ft4_cabi.
    if (nfqso_in >= nfa .and. nfqso_in <= nfb) then
       nfqso = nfqso_in
    else
       nfqso = (nfa + nfb) / 2
    end if

    ! Fixed arguments, matching upstream decoder.f90:1245. nutc is a slot label used
    ! only for the diagnostic dump the headless build excised, so 0 is safe. iwspr
    ! now comes from the caller: 0 = FST4 QSO mode, 1 = FST4W beacon mode (see the
    ! banner for what the missing hashed-callsign table costs FST4W).
    nutc         = 0
    nexp_decode  = 0
    ! ⭐ ntol MEANS SOMETHING DIFFERENT IN FST4W, and getting it wrong makes the
    ! mode look broken rather than deaf.
    !
    ! For FST4 (iwspr=0) the search runs across the caller's nfa..nfb and ntol only
    ! sizes the AP window, so a narrow 20 Hz value is right.
    !
    ! For FST4W (iwspr=1) fst4_decode.f90:298-302 THROWS AWAY the caller's nfa/nfb
    ! and rebuilds the whole search from nfqso +/- ntol:
    !     nfa = max(100, nfqso-ntol-100)
    !     fa  = max(100, nint(nfqso+1.5*baud-ntol))
    ! With ntol=20 that is a 40 Hz slot around the band centre, so a beacon
    ! anywhere else in the passband is never looked at. Measured: a 1500 Hz FST4W
    ! signal at -10 dB, which stock `jt9 -W` decodes easily, produced NOTHING here
    ! until this was fixed.
    !
    ! So for FST4W ntol is derived from the band the caller actually asked for,
    ! which makes nfa..nfb mean the same thing in both modes.
    if (iwspr_in == 1) then
       ntol = max(20, (nfb - nfa) / 2)
    else
       ntol = 20
    end if
    emedelay     = 0.0
    lagain       = .false.
    lapcqonly    = .false.
    iwspr        = iwspr_in
    lprinthash22 = .false.

    decoder%callback => fst4_collect_cb
    call decoder%decode(fst4_collect_cb, iwave_l, nutc, nqso_progress, &
         nfa, nfb, nfqso, ndepth_l, ntrperiod, nexp_decode, ntol, &
         emedelay, lagain, lapcqonly, mycall_f, hiscall_f, iwspr, lprinthash22)
    deallocate(iwave_l)

    ! ncopy == max_out means the cap was hit and decodes were dropped: raise
    ! FST4_MAXDEC and the Rust-side MAX_DECODES together.
    ncopy = min(gs4_count, max_out)
    do i = 1, ncopy
       out(i)%sync = gs4_results(i)%sync
       out(i)%snr  = gs4_results(i)%snr
       out(i)%dt   = gs4_results(i)%dt
       out(i)%freq = gs4_results(i)%freq
       out(i)%nap  = gs4_results(i)%nap
       out(i)%qual = gs4_results(i)%qual
       do j = 1, 38
          out(i)%message(j) = c_null_char
       end do
       n = len_trim(gs4_results(i)%message)
       if (n > 37) n = 37
       do j = 1, n
          out(i)%message(j) = gs4_results(i)%message(j:j)
       end do
    end do

    ndec = gs4_count
  end function fst4_decode_frame


  !-------------------------------------------------------------------------
  ! fst4_encode_msg : message text -> the 160 FST4 channel symbols.
  !
  !   msg       : message text (C string, <= 37 chars)
  !   msg_len   : length of msg
  !   iwspr     : 0 = FST4 (77-bit QSO message), 1 = FST4W (50-bit beacon)
  !   itone_out : caller buffer, FST4_NN entries, values 0..3
  !   returns   : FST4_NN on success, -1 if the message will not pack
  !
  ! Straight into the vendored genfst4 - the same file whose get_fst4_tones_from_bits
  ! ENTRY point the decoder already calls, so encode and decode share one generator.
  !
  ! ⭐ iwspr PICKS THE CODE, not just the message format: 0 selects LDPC(240,101)
  ! with a 77-bit payload, 1 selects LDPC(240,74) with the 50-bit WSPR-style
  ! payload. Both produce 160 symbols, so a wrong iwspr yields a perfectly
  ! well-formed transmission that the other side's decoder cannot read.
  !-------------------------------------------------------------------------
  function fst4_encode_msg(msg, msg_len, iwspr, itone_out) result(nsym_out) &
       bind(C, name="fst4_encode_msg")
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len, iwspr
    integer(c_int),         intent(out) :: itone_out(FST4_NN)
    integer(c_int) :: nsym_out

    character(len=37) :: msg37, msgsent37
    character(len=101) :: msgbits
    integer :: itone(FST4_NN)
    integer :: i, n, ichk, iwspr_l

    nsym_out = -1
    if (iwspr /= 0 .and. iwspr /= 1) return

    msg37 = ' '
    n = min(msg_len, 37)
    do i = 1, n
       if (msg(i) == c_null_char) exit
       msg37(i:i) = msg(i)
    end do

    ichk = 0
    iwspr_l = iwspr
    itone = -1
    call genfst4(msg37, ichk, msgsent37, msgbits, itone, iwspr_l)
    ! genfst4 leaves itone untouched when pack77 refuses the message. Every real
    ! symbol is 0..3, so a surviving -1 means nothing was generated.
    if (any(itone(1:FST4_NN) < 0)) return

    itone_out(1:FST4_NN) = itone(1:FST4_NN)
    nsym_out = FST4_NN
  end function fst4_encode_msg

  !-------------------------------------------------------------------------
  ! fst4_gen_wave : channel symbols -> real audio.
  !
  !   itone     : the 160 symbols from fst4_encode_msg
  !   nsym      : symbol count (FST4_NN)
  !   ntrperiod : T/R period, seconds - sets the symbol duration
  !   hmod      : tone-spacing multiplier, 1 | 2 | 4 (upstream's x2/x4 Tone Spacing)
  !   fsample   : output sample rate (Hz)
  !   f0        : NOMINAL audio carrier (Hz) - see the offset note below
  !   wave_out  : caller buffer (capacity nwave_cap)
  !   returns   : samples produced (nsym*nsps), or -1 on refusal
  !
  ! Delegates to the vendored gen_fst4wave, which is NOT a plain MFSK generator
  ! like Q65's: it applies a GFSK frequency-deviation pulse (BT=2.0) spanning three
  ! symbols, plus raised-cosine ramps over the first and last quarter-symbol. That
  ! shaping is why FST4 is clean enough for the LF/MF bands it lives on, and it is
  ! why this calls upstream rather than synthesising here.
  !
  ! ⭐ THE 1.5-TONE OFFSET, and why it looks redundant but is not.
  ! gen_fst4wave internally shifts DOWN by 1.5 tone spacings:
  !     dphi = dphi + twopi*(f0 - 1.5*hmod/tsym)*dt
  ! and both of upstream's callers shift UP by the same amount before calling:
  !     if(!m_tune) f0 += 1.5*dfreq;            (mainwindow.cpp:12703)
  ! The two cancel, which is the point: without them the 4-tone constellation would
  ! be CENTRED on f0, and with them the LOWEST tone sits at f0 - the convention the
  ! decoder reports frequency in. We add it here so this ABI's `f0` means the same
  ! thing it does for every other mode: where the signal is reported.
  !
  ! Note upstream skips the offset for TUNE (a single carrier, no constellation to
  ! centre). We have no tune path through here - Tune is a separate carrier.
  !-------------------------------------------------------------------------
  function fst4_gen_wave(itone, nsym, ntrperiod, hmod, fsample, f0, &
       wave_out, nwave_cap) result(nwave_out) bind(C, name="fst4_gen_wave")
    integer(c_int),        intent(in)    :: itone(*)
    integer(c_int), value, intent(in)    :: nsym, ntrperiod, hmod
    real(c_float),  value, intent(in)    :: fsample, f0
    real(c_float),         intent(inout) :: wave_out(*)
    integer(c_int), value, intent(in)    :: nwave_cap
    integer(c_int) :: nwave_out

    integer :: nsps, nwave, ip, icmplx, itone_l(FST4_NN), hmod_l
    real    :: fs_l, f0_l, dfreq
    complex :: cwave(1)                      ! icmplx=0: never written

    nwave_out = -1
    if (nsym /= FST4_NN) return
    if (hmod /= 1 .and. hmod /= 2 .and. hmod /= 4) return
    ip = fst4_period_index(ntrperiod)
    if (ip < 1) return

    nsps  = FST4_NSPS(ip)
    nwave = nsym * nsps
    if (nwave_cap < nwave) return

    ! Tone spacing at the OUTPUT rate; the caller-side half of the cancelling pair.
    dfreq = real(hmod) * fsample / real(nsps)
    itone_l(1:FST4_NN) = itone(1:FST4_NN)
    hmod_l = hmod
    fs_l   = fsample
    f0_l   = f0 + 1.5 * dfreq
    icmplx = 0

    ! gen_fst4wave writes (nsym+2)*nsps of dphi internally but emits exactly
    ! nsym*nsps samples (its output loop runs j=nsps..(nsym+1)*nsps-1).
    call gen_fst4wave(itone_l, nsym, nsps, nwave, fs_l, hmod_l, f0_l, &
                      icmplx, cwave, wave_out(1:nwave))
    nwave_out = nwave
  end function fst4_gen_wave

  !-------------------------------------------------------------------------
  ! fst4_period_index : 1..FST4_NPERIODS for a supported T/R period, else -1.
  ! Reject, never clamp - the same rule the decode side follows.
  !-------------------------------------------------------------------------
  integer function fst4_period_index(ntrperiod)
    integer, intent(in) :: ntrperiod
    integer :: i
    fst4_period_index = -1
    do i = 1, FST4_NPERIODS
       if (FST4_PERIODS(i) == ntrperiod) then
          fst4_period_index = i
          return
       end if
    end do
  end function fst4_period_index

  !-------------------------------------------------------------------------
  ! c_to_fstr12_fst4 : marshal a NUL/space-terminated C string into character(12).
  ! Module-scoped and suffixed so it cannot collide with ft4_cabi/ft8_cabi's copies.
  !-------------------------------------------------------------------------
  subroutine c_to_fstr12_fst4(cstr, fstr)
    character(kind=c_char), intent(in)  :: cstr(*)
    character(len=12),      intent(out) :: fstr
    integer :: i
    fstr = ' '
    do i = 1, 12
       if (cstr(i) == c_null_char) exit
       fstr(i:i) = cstr(i)
    end do
  end subroutine c_to_fstr12_fst4

end module fst4_cabi
