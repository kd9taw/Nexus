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
