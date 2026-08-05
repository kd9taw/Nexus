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

  ! MSK144 channel symbols per frame: s8 + 48 bits + s8 + 80 bits = 144, which at
  ! 2000 baud is a 72 ms message (genmsk_128_90.f90:2). Upstream's
  ! NUM_MSK144_SYMBOLS. The MSK40 shorthand form is 40 of these.
  integer, parameter :: MSK144_NN = 144

  ! The fixed centre and the frequency-ERROR budget around it. Upstream pins both
  ! spin boxes to 1500, clamps RX to 1400..1600 (mainwindow.cpp:8097-8099), and
  ! defaults Ftol_MSK144 to 50 from the set {20,50,100,200} (mainwindow.cpp:1444,
  ! :8113). Matches our own TX: msk144::TX_CENTRE_HZ is 1500 and gen_wave ignores
  ! the operator's offset, because a 1000 Hz-wide signal has nowhere to move.
  ! ⚠️ NTOL is NOT a passband — see the long note in msk144_decode_frame. If a
  ! Settings control ever exposes it, it takes the upstream set and clamps here.
  integer, parameter :: MSK144_NFQSO_HZ  = 1500
  integer, parameter :: MSK144_NFQSO_MIN = 1400
  integer, parameter :: MSK144_NFQSO_MAX = 1600
  integer, parameter :: MSK144_NTOL_HZ   = 50

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
  !   nfa, nfb   : ACCEPTED FOR ABI UNIFORMITY AND OTHERWISE UNUSED. Every other
  !                mode searches a band; MSK144 sits at one fixed centre, so the
  !                width the caller asks for says nothing about how far off
  !                frequency the far station can be. See the note in the body.
  !   ndepth     : 1..3 (3 = deepest; <=0 defaults to 3)
  !   mycall     : NUL/space-terminated callsign for AP + hashed shorthand
  !   hiscall    : NUL/space-terminated callsign (may be empty)
  !   nfqso_in   : QSO/RX audio freq (Hz). Honoured only inside 1400..1600;
  !                anything else (including 0) => the mode's own 1500 Hz centre.
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

    ! ⭐ nfa/nfb DO NOT SIZE THE SEARCH HERE, AND THAT IS DELIBERATE. mskrtd takes
    ! a centre and a tolerance, but for MSK144 neither is a passband: the mode
    ! lives at ONE fixed centre. The signal is 1000 Hz wide (tones at centre ±500)
    ! and fills a normal SSB passband, so there is nowhere to move it — which is
    ! why our own gen_wave ignores the operator's TX offset, and why upstream pins
    ! BOTH spin boxes to 1500 and clamps the RX one to 1400..1600
    ! (mainwindow.cpp:8097-8099). ntol is therefore a frequency-ERROR budget — rig
    ! offset and Doppler — and upstream defaults it to 50, operator-settable from
    ! {20,50,100,200} (mainwindow.cpp:1444, :8113). Its widest possible search is
    ! 1200..1800.
    !
    ! Deriving both from the caller's band instead was a category error twice
    ! over. At the default 200..2900 it gave ntol=1350, and:
    !   * msk144sync searches `2*nint(ntol/delf)+1` bins (msk144sync.f90:56) — a
    !     search 27x wider than upstream's, measured at 2.55 s per 15 s period
    !     against 0.10 s after this change. That is the decode that looked hung.
    !   * msk144spd gates detections on `abs(detfer(il)) <= ntol`
    !     (msk144spd.f90:131,145), so it would accept one a kilohertz off
    !     frequency — not an MSK144 contact, and never reported by WSJT-X.
    !
    ! The FST4W precedent does NOT transfer, though it is why this looks risky: a
    ! pinned ntol=20 made FST4W look dead because a BEACON sits wherever the
    ! operator put it, so the centre was genuinely unknown. Here the centre is
    ! knowable exactly, so pinning the tolerance costs nothing.
    !
    ! nfqso_in is honoured only within upstream's clamp. An operator whose RX
    ! offset still carries an FT8 habit (1200 is typical) must not lose the mode
    ! by dragging a ±50 Hz window off the only frequency the signal is ever on.
    if (nfqso_in >= MSK144_NFQSO_MIN .and. nfqso_in <= MSK144_NFQSO_MAX) then
       nfqso = nfqso_in
    else
       nfqso = MSK144_NFQSO_HZ
    end if
    ntol = MSK144_NTOL_HZ

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

  !-------------------------------------------------------------------------
  ! msk144_encode_msg : message text -> MSK144 channel symbols (bits, 0 or 1).
  !
  !   msg        : message text (C string, <= 37 chars)
  !   msg_len    : length of msg
  !   itone_out  : caller buffer, MSK144_NN entries
  !   returns    : 144 for a full message, 40 for an MSK40 SHORTHAND, -1 on failure
  !
  ! Straight into the vendored genmsk_128_90 - the same routine the DECODER already
  ! calls to regenerate a candidate for comparison, so encode and decode share one
  ! generator and cannot drift apart.
  !
  ! ⭐ MSK144 IS 2-FSK, not 4- or 65-. itone is a BIT, 0 or 1. The "MSK" is in how
  ! those bits are carried: minimum shift keying is continuous-phase FSK with
  ! modulation index 0.5, so the two tones sit baud/2 = 1000 Hz apart. That spacing
  ! is not a free parameter - it is what makes the modulation MSK.
  !
  ! ⭐ SHORTHAND RETURNS 40, NOT 144. genmsk_128_90 emits a 40-symbol MSK40 frame
  ! for the short "<Call_1 Call2> Rpt" forms, signalled by itone(41) < 0 - exactly
  ! how upstream detects it (msk144sim.f90:55, mainwindow.cpp:12772). A caller that
  ! assumes 144 would transmit 104 symbols of uninitialised memory.
  !-------------------------------------------------------------------------
  function msk144_encode_msg(msg, msg_len, itone_out) result(nsym_out) &
       bind(C, name="msk144_encode_msg")
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len
    integer(c_int),         intent(out) :: itone_out(MSK144_NN)
    integer(c_int) :: nsym_out

    character(len=37) :: msg37
    character(len=37) :: msgsent37
    integer :: itone(MSK144_NN)
    integer :: i, n, ichk, itype

    nsym_out = -1
    msg37 = ' '
    n = min(msg_len, 37)
    do i = 1, n
       if (msg(i) == c_null_char) exit
       msg37(i:i) = msg(i)
    end do

    ichk = 0
    itype = 0
    itone = -1
    call genmsk_128_90(msg37, ichk, msgsent37, itone, itype)

    ! itype <= 0 means the packer refused the message outright.
    if (itype <= 0) return

    ! Upstream's own shorthand test. A full frame fills all 144; a short one leaves
    ! everything from 41 on at the -1 we seeded.
    if (itone(41) < 0) then
       if (any(itone(1:40) < 0)) return
       itone_out(1:40) = itone(1:40)
       itone_out(41:MSK144_NN) = 0
       nsym_out = 40
    else
       if (any(itone(1:MSK144_NN) < 0)) return
       itone_out(1:MSK144_NN) = itone(1:MSK144_NN)
       nsym_out = MSK144_NN
    end if
  end function msk144_encode_msg

end module msk144_cabi
