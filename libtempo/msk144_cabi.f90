! Nexus: C ABI wrapper for native MSK144 decode, built on the vendored WSJT-X GPL
! modem sources (mskrtd + the msk144/msk40 family).
!
! MSK144 is the METEOR-SCATTER mode: 72 ms frames (NSPM=864 samples @ 12 kHz)
! repeated continuously through a T/R period, so a single ionised meteor trail
! lasting a tenth of a second can carry a whole message. That makes its decoder
! shaped differently from every other mode in this tree.
!
! ⭐ IT IS A SLIDING-WINDOW DECODER, NOT A ONE-SHOT FRAME DECODER.
!   `mskrtd` ("real-time decoder") analyses ONE 7168-sample block (0.597 s) per
!   call and is driven at half-block (3584-sample, ~0.3 s) increments across the
!   period. Upstream does this from hspec.f90:92-99 as audio streams in; the
!   standalone decode_msk144.f90 does the same over a captured buffer. This
!   wrapper follows decode_msk144: it owns the slide internally, so the caller
!   still gets the familiar "hand over one period, get every decode back" shape.
!
! ⭐ ITS OUTPUT IS A FORMATTED TEXT LINE, not a struct.
!   mskrtd writes `format(i6.6,i4,f5.1,i5,a4,a37,a1)` into a character(80) and
!   signals "nothing this block" with a leading NUL. This wrapper reads that back
!   with an internal READ using the same format — exact field recovery, not string
!   scraping — and fills the interop struct. If upstream ever changes format 1021,
!   FMT_1021 below changes with it.
!
! ⭐ nutc IS THE PERIOD LABEL, AND IT MUST DIFFER BETWEEN PERIODS.
!   mskrtd dupe-checks decodes against the previous message (msglast) and resets
!   that check at mskrtd.f90:88 on `nutc00.ne.nutc0 .or. tsec.lt.tsec0`.
!
!   CORRECTION (KD9TAW, 2026-07-27): an earlier version of this banner claimed the
!   second disjunct never fires because tsec0 is assigned once at :60 and never
!   advances. THAT WAS WRONG. `999 tsec0=tsec` at mskrtd.f90:238 is a LABELLED
!   assignment — easy to miss when grepping for an assignment at the start of a
!   line — and every exit path reaches it: the low-rms bail (`go to 999` at :103),
!   the no-decode bail at :184, and fall-through from :237. tsec0 therefore holds
!   the PREVIOUS call's block time and advances on every call.
!
!   So the reset DOES self-clear at a period boundary on its own: within a period
!   tsec rises monotonically as the slide below advances ipos, and at the boundary
!   tsec restarts near 0 against a stored ~14.4, firing `tsec.lt.tsec0`.
!
!   Passing a distinct nutc per period is still correct and still required — it is
!   the UTC field of the output line, it is the other half of the reset, and
!   tsec0 is ALSO read at :212 and :230 as the third disjunct of both dupe tests.
!   Only the stated mechanism was wrong, not the contract.

! RX-ONLY, DELIBERATELY. There is no msk144 encode or gen_wave here. The Rust
! ModeKind reports `Capabilities { tx: false }` so modes::tx_mode() refuses to
! hand it to the transmit path. Adding TX means adding those entry points AND
! flipping that flag AND passing the FT-mode TX hard gate.
!
! (genmsk_128_90 and genmsk40 ARE compiled in regardless: mskrtd's signal-quality
! and MSK40 paths call them to regenerate a candidate for comparison, so they are
! on the RECEIVE path despite the names.)

module msk144_cabi
  use iso_c_binding
  implicit none

  integer, parameter :: MSK144_NZ      = 7168              ! mskrtd analysis block
  integer, parameter :: MSK144_NSTEP   = MSK144_NZ / 2      ! half-block slide (~0.3 s)
  integer, parameter :: MSK144_NMAX_MAX = 30 * 12000        ! ceiling: 30 s @ 12 kHz
  integer, parameter :: MSK144_MAXDEC  = 100

  ! The T/R periods WSJT-X offers for MSK144. 15 s is the 6 m workhorse.
  integer, parameter :: MSK144_NPERIODS = 4
  integer, parameter :: MSK144_PERIODS(MSK144_NPERIODS) = [5, 10, 15, 30]

  ! mskrtd.f90:220 format 1021 — the contract this wrapper reads back.
  character(len=*), parameter :: FMT_1021 = '(i6,i4,f5.1,i5,a4,a37)'

  ! Interop result struct. Layout MUST match msk144_decode_t in libtempo.h, and is
  ! byte-compatible with the ft8/ft4/fst4/q65 records (64 bytes, 4-byte aligned).
  type, bind(C) :: msk144_decode_t
     ! MSK144 reports no sync metric — mskrtd's output line carries only UTC, SNR,
     ! dt, frequency, a decode-type symbol and the message. Always 0.0; kept so the
     ! record stays the same shape as every other mode's.
     real(c_float)          :: sync
     integer(c_int)         :: snr
     real(c_float)          :: dt
     real(c_float)          :: freq
     character(kind=c_char) :: message(38)
     ! Decode type, from mskrtd's `decsym`: 0 = normal frame-averaged decode,
     ! 1 = '&' (mskspd — a single-frame "fast" decode off one bright ping),
     ! 2 = '^' (a long average across many frames). Worth surfacing: a '&' decode
     ! came off one meteor, a '^' came from patiently stacking a whole period.
     integer(c_int)         :: dtype
     integer(c_int)         :: reserved
  end type msk144_decode_t

contains

  !-------------------------------------------------------------------------
  ! msk144_decode_frame : decode EVERY MSK144 signal in one T/R period.
  !
  !   iwave      : ntrperiod*12000 int16 audio samples @ 12 kHz
  !   ntrperiod  : 5, 10, 15 or 30 (seconds). Anything else => -1.
  !   nutc       : per-period label. MUST DIFFER between periods — see the banner.
  !   nfa, nfb   : frequency search band edges (Hz). MSK144 searches nfqso +/- ntol
  !                rather than a range, so ntol is derived from the width asked for.
  !   ndepth     : 1..3 (3 = deepest; <=0 defaults to 3)
  !   mycall     : NUL/space-terminated callsign for AP + hashed shorthand
  !   hiscall    : NUL/space-terminated callsign (may be empty)
  !   nfqso_in   : QSO/RX audio freq (Hz). 0 / out of band => band centre.
  !   out        : caller array of msk144_decode_t (capacity max_out)
  !   max_out    : capacity of out
  !
  !   Returns the number of decodes found (>= 0), or -1 on error.
  !
  ! NOT thread-safe: mskrtd keeps process-global SAVE state (the analytic-signal
  ! buffer, the dupe-check pair, the MSK40 hash array and the recent-shorthand
  ! ring). Classified in modem-state-manifest.toml.
  !-------------------------------------------------------------------------
  function msk144_decode_frame(iwave, ntrperiod, nutc, nfa, nfb, ndepth, &
       mycall, hiscall, nfqso_in, out, max_out) result(ndec) &
       bind(C, name="msk144_decode_frame")
    integer(c_int16_t),     intent(in)  :: iwave(*)
    integer(c_int), value,  intent(in)  :: ntrperiod, nutc
    integer(c_int), value,  intent(in)  :: nfa, nfb, ndepth
    integer(c_int), value,  intent(in)  :: nfqso_in, max_out
    character(kind=c_char), intent(in)  :: mycall(*)
    character(kind=c_char), intent(in)  :: hiscall(*)
    type(msk144_decode_t),  intent(out) :: out(*)
    integer(c_int)                      :: ndec

    integer(kind=2)   :: block(MSK144_NZ)
    character(len=12) :: mycall_f, hiscall_f
    character(len=80) :: line
    character(len=4)  :: decsym
    character(len=37) :: msg
    real(kind=8)      :: pcoeffs(5)
    real              :: tsec, tdec
    integer           :: npts, nfqso, ntol, ndepth_l
    integer           :: ipos, j, n, nsnr, nfreq, nutc_r, ios
    logical(kind=1)   :: bshmsg, btrain, bswl

    ndec = 0
    if (max_out <= 0) return

    ! REJECT rather than clamp: the slide below reads ntrperiod*12000 samples, so a
    ! period the caller did not size for would run off the end of iwave.
    if (.not. any(MSK144_PERIODS == ntrperiod)) then
       ndec = -1
       return
    end if
    npts = ntrperiod * 12000

    call c_to_fstr12_msk(mycall, mycall_f)
    call c_to_fstr12_msk(hiscall, hiscall_f)

    ndepth_l = ndepth
    if (ndepth_l <= 0) ndepth_l = 3

    ! MSK144 has no nfa..nfb search: mskrtd takes a centre and a tolerance. Derive
    ! both from the band the caller asked for so nfa/nfb mean the same thing here
    ! as in every other mode's ABI. (FST4W needed the identical treatment; there,
    ! a pinned ntol=20 made the mode look dead because the beacon sat outside a
    ! 40 Hz window.)
    if (nfqso_in >= nfa .and. nfqso_in <= nfb) then
       nfqso = nfqso_in
    else
       nfqso = (nfa + nfb) / 2
    end if
    ntol = max(20, (nfb - nfa) / 2)

    ! Fixed arguments:
    !   bshmsg  .false. — MSK144 SHORTHAND (MSK40) messages off, matching WSJT-X's
    !           default. The msk40 family is still linked because mskrtd calls it
    !           unconditionally in source; this flag is what gates it at run time.
    !   btrain  .false. — phase-equalizer training off. Its only output was the
    !           .pcoeff dump the headless surgery removed
    !           (msk144signalquality.f90).
    !   bswl    .false. — SWL mode (log everything heard, including messages not
    !           addressed to us) off.
    !   pcoeffs zero — no phase equalization, which is what btrain=.false. implies.
    bshmsg  = .false.
    btrain  = .false.
    bswl    = .false.
    pcoeffs = 0.0d0

    ! The slide. Mirrors decode_msk144.f90:36-45 exactly: 7168-sample blocks at
    ! 3584-sample steps, tsec measured from the START of the period so mskrtd's
    ! reported dt lands in period-relative seconds.
    do ipos = 1, npts - MSK144_NZ + 1, MSK144_NSTEP
       block = iwave(ipos:ipos + MSK144_NZ - 1)
       tsec = real(ipos - 1) / 12000.0

       line = ' '
       call mskrtd(block, nutc, tsec, ntol, nfqso, ndepth_l, mycall_f, &
            hiscall_f, bshmsg, btrain, pcoeffs, bswl, ' ', line)

       ! A leading NUL means "no decode from this block".
       if (line(1:1) == char(0)) cycle
       if (ndec >= max_out) cycle

       read(line, FMT_1021, iostat=ios) nutc_r, nsnr, tdec, nfreq, decsym, msg
       if (ios /= 0) cycle

       ndec = ndec + 1
       out(ndec)%sync     = 0.0
       out(ndec)%snr      = nsnr
       out(ndec)%dt       = tdec
       out(ndec)%freq     = real(nfreq)
       out(ndec)%dtype    = decsym_code(decsym)
       out(ndec)%reserved = 0
       do j = 1, 38
          out(ndec)%message(j) = c_null_char
       end do
       n = len_trim(msg)
       if (n > 37) n = 37
       do j = 1, n
          out(ndec)%message(j) = msg(j:j)
       end do
    end do
  end function msk144_decode_frame

  !-------------------------------------------------------------------------
  ! decsym_code : mskrtd's decode-type symbol -> a small int for the C struct.
  ! mskrtd.f90:16 documents "&" for mskspd and "^" for long averages.
  !-------------------------------------------------------------------------
  integer function decsym_code(decsym)
    character(len=*), intent(in) :: decsym
    if (index(decsym, '&') > 0) then
       decsym_code = 1
    else if (index(decsym, '^') > 0) then
       decsym_code = 2
    else
       decsym_code = 0
    end if
  end function decsym_code

  !-------------------------------------------------------------------------
  ! c_to_fstr12_msk : marshal a NUL/space-terminated C string into character(12).
  ! Suffixed so it cannot collide with the other *_cabi copies.
  !-------------------------------------------------------------------------
  subroutine c_to_fstr12_msk(cstr, fstr)
    character(kind=c_char), intent(in)  :: cstr(*)
    character(len=12),      intent(out) :: fstr
    integer :: i
    fstr = ' '
    do i = 1, 12
       if (cstr(i) == c_null_char) exit
       fstr(i:i) = cstr(i)
    end do
  end subroutine c_to_fstr12_msk

end module msk144_cabi
