! JTTY - Nexus's C ABI over the vendored WSJT-X JTTY modem
! Copyright (C) 2026 KD9TAW
!
! This file is part of Nexus.
!
! Nexus is free software: you can redistribute it and/or modify it under the
! terms of the GNU General Public License as published by the Free Software
! Foundation, version 3 of the License only (GPL-3.0-only).
!
! Distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
! without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
! PARTICULAR PURPOSE.  See the GNU General Public License for more details.
!
! ---------------------------------------------------------------------------
! JTTY is WSJT-X's untimed keyboard mode: 4-GFSK at 31.25 baud, 59-symbol
! frames (13 sync + 46 channel symbols, 1.888 s at 12 kHz), each carrying one
! 34-bit atom under a CRC-12 and a K=10 tail-biting convolutional code; a
! message is 1..16 frames. The modem sources are upstream's, unchanged, under
! vendor/wsjtx/lib/jtty/. This file is Nexus's own and only marshals.
!
! What it adds over upstream's own entries, and why:
!
!   jtty_encode_text  -> genjtty_profile, the entry WSJT-X's transmitter calls
!                        (widgets/mainwindow_jtty.cpp), with its end-of-message
!                        choice. Adds a length limit, a capacity check on the
!                        caller's tone buffer, the normalised text back (what
!                        will actually be sent), and tells an empty message
!                        from one that cannot be encoded, which genjtty_profile
!                        reports alike (nsym=0).
!   jtty_encode_atoms -> genjtty_atoms_c, upstream's own C entry. Adds the
!                        capacity check; upstream writes up to 59*16 tones
!                        into an assumed-size buffer.
!   jtty_pack_text    -> pack_jtty, the packing genjtty_profile does before
!                        the CRC and the code, returned as one 34-bit source
!                        word per frame: the form upstream's spec publishes
!                        its vectors in. The same limits and normalised text
!                        as jtty_encode_text, plus the end-of-message choice.
!   jtty_unpack_frames-> unpack_jtty over source words, rendered as the
!                        receiver shows a message (display_message_text), and
!                        says whether every word was one the receiver keeps.
!   jtty_gen_wave     -> gen_jttywave at 12 kHz: nsps 384, bt 2.0, the
!                        48 kHz call WSJT-X makes (nsps 1536) scaled to
!                        Nexus's audio rate. Refuses a symbol count that is
!                        not whole frames and any tone outside 0..3.
!   jtty_take_updates -> jtty_rx_take_updates, whose seven parallel arrays
!                        become one jtty_update_t record per update.
!
! The rest of the receive API is upstream's bind(C) jtty_rx_* functions in
! jtty_receive_context.f90, used as they are; libtempo.h documents them.
!
! ⚠️ gen_jttywave caches its pulse table under a key that is a SUM of its
! parameters (nsym+nsps+bt+fsample), so two parameter sets can collide. This
! wrapper pins nsps, bt and the sample rate; the manifest's gen_jttywave rows
! show why the decoder's own calls (6000 Hz) cannot meet these.
!
! NOT thread-safe, and no state of its own: every call reaches module and
! SAVE state in the vendored modem (modem-state-manifest.toml, GROUP O). Run
! every entry, including upstream's jtty_rx_*, under the modem lock.
! ---------------------------------------------------------------------------

module jtty_cabi
  use iso_c_binding
  implicit none

  integer, parameter :: JTTY_NSPS          = 384    ! samples/symbol at 12 kHz: 31.25 baud
  integer, parameter :: JTTY_FRAME_SYMBOLS = 59     ! 13 sync + 46 channel symbols
  integer, parameter :: JTTY_MAX_FRAMES    = 16     ! jtty_mod's MAX_FRAMES
  integer, parameter :: JTTY_MAX_TONES     = JTTY_FRAME_SYMBOLS * JTTY_MAX_FRAMES  ! 944
  integer, parameter :: JTTY_TEXT_LEN      = 80     ! upstream's message length
  integer, parameter :: JTTY_UPDATE_TEXT   = 84     ! text(80) + NUL fill to 8-byte size
  integer, parameter :: JTTY_TAKE_BATCH    = 30     ! what WSJT-X's own decoder takes per call

  ! Return codes shared by the encode and wave entries.
  integer(c_int), parameter :: JTTY_ERR_ARGS   = -1  ! a length, count, tone or capacity refused
  integer(c_int), parameter :: JTTY_ERR_ENCODE = -2  ! the modem could not encode the text

  ! One receive update. Layout MUST match jtty_update_t in libtempo.h (120 bytes,
  ! 8-byte aligned, no implicit padding). Times are seconds on the caller's sample
  ! timeline: sample index / 12000, the index being the one jtty_rx_process saw.
  type, bind(C) :: jtty_update_t
     integer(c_int64_t)     :: message_id   ! stable across a message's growing updates
     real(c_double)         :: start_s      ! the message's first frame
     real(c_double)         :: latest_s     ! its latest frame
     real(c_float)          :: freq         ! audio frequency, Hz
     integer(c_int)         :: terminal     ! 0 growing, 1 complete, 2 expired, 3 reception ended
     integer(c_int)         :: snr          ! dB, the message's first frame
     character(kind=c_char) :: text(JTTY_UPDATE_TEXT)  ! the text so far, NUL-filled
  end type jtty_update_t

contains

  !-------------------------------------------------------------------------
  ! jtty_encode_text : message text -> channel tones (0..3).
  !
  !   msg, msg_len : the text, at most JTTY_TEXT_LEN bytes (a NUL ends it early)
  !   profile      : exchange profile, 0 unknown, 1 Field Day, 2 RTTY Roundup
  !   is_final     : 1 sets end-of-message on the last frame, as a whole message
  !                  does; 0 leaves it clear, for a segment of text still being
  !                  typed (WSJT-X's live entry). Anything else is refused.
  !   itone_out    : caller buffer, capacity max_tones (JTTY_MAX_TONES always fits)
  !   sent         : caller buffer, capacity sent_cap >= JTTY_TEXT_LEN+1; gets the
  !                  normalised text (upper case, single spaces, unknown
  !                  characters as '#'), NUL-terminated
  !   returns      : nsym, a whole number of frames (59 each); 0 for an empty
  !                  message; JTTY_ERR_ARGS for a refused length or capacity
  !                  (nothing written); JTTY_ERR_ENCODE if the modem cannot encode the
  !                  text (more than 16 frames, an unknown profile, an RTTY
  !                  exchange whose serial will not parse). For an empty or
  !                  unencodable message, sent still holds the normalised text.
  !-------------------------------------------------------------------------
  function jtty_encode_text(msg, msg_len, profile, is_final, itone_out, max_tones, sent, sent_cap) &
       result(nsym_out) bind(C, name="jtty_encode_text")
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len, profile, is_final, max_tones, sent_cap
    integer(c_int),         intent(out) :: itone_out(*)
    character(kind=c_char), intent(out) :: sent(*)
    integer(c_int) :: nsym_out

    interface
       subroutine genjtty_profile(umsg, exchange_profile, itone, nsym, frame_starts, is_final)
         use jtty_mod, only: MAX_FRAMES
         character(len=80), intent(inout) :: umsg
         integer, intent(in) :: exchange_profile
         integer, intent(out) :: itone(59*MAX_FRAMES), nsym
         integer, intent(out), optional :: frame_starts(MAX_FRAMES)
         integer, intent(in), optional :: is_final
       end subroutine genjtty_profile
    end interface

    character(len=JTTY_TEXT_LEN) :: umsg
    integer :: itone(JTTY_MAX_TONES), nsym, final, i, n

    nsym_out = JTTY_ERR_ARGS
    if (msg_len < 0 .or. msg_len > JTTY_TEXT_LEN) return
    if (sent_cap < JTTY_TEXT_LEN + 1) return
    if (is_final /= 0 .and. is_final /= 1) return

    umsg = ' '
    do i = 1, msg_len
       if (msg(i) == c_null_char) exit
       umsg(i:i) = msg(i)
    end do

    itone = 0
    nsym = 0
    final = is_final
    call genjtty_profile(umsg, int(profile), itone, nsym, is_final=final)
    if (nsym > 0 .and. max_tones < nsym) return

    n = len_trim(umsg)
    do i = 1, n
       sent(i) = umsg(i:i)
    end do
    sent(n + 1) = c_null_char

    if (nsym <= 0) then
       nsym_out = 0
       if (n > 0) nsym_out = JTTY_ERR_ENCODE
       return
    end if
    itone_out(1:nsym) = itone(1:nsym)
    nsym_out = nsym
  end function jtty_encode_text

  !-------------------------------------------------------------------------
  ! jtty_encode_atoms : structured atoms -> channel tones, one frame per atom.
  !
  !   atoms, natoms : upstream's jtty_source_atom_c descriptors (1..16)
  !   itone_out     : caller buffer, capacity max_tones
  !   status        : upstream's status: 0 ok, 1 invalid descriptor,
  !                   2 unknown ARRL/RAC section
  !   returns       : nsym (natoms*59), 0 when upstream refuses the atoms
  !                   (status says why), or JTTY_ERR_ARGS when the buffer is
  !                   too small (nothing written)
  !-------------------------------------------------------------------------
  function jtty_encode_atoms(atoms, natoms, itone_out, max_tones, status) &
       result(nsym_out) bind(C, name="jtty_encode_atoms")
    use jtty_mod, only: jtty_source_atom_c
    type(jtty_source_atom_c), intent(in)  :: atoms(*)
    integer(c_int), value,    intent(in)  :: natoms, max_tones
    integer(c_int),           intent(out) :: itone_out(*)
    integer(c_int),           intent(out) :: status
    integer(c_int) :: nsym_out

    interface
       subroutine genjtty_atoms_c(c_atoms, natoms, itone, nsym, status) bind(C, name='genjtty_atoms_c')
         use iso_c_binding, only: c_int
         use jtty_mod, only: jtty_source_atom_c
         type(jtty_source_atom_c), intent(in) :: c_atoms(*)
         integer(c_int), value, intent(in) :: natoms
         integer(c_int), intent(out) :: itone(*)
         integer(c_int), intent(out) :: nsym
         integer(c_int), intent(out) :: status
       end subroutine genjtty_atoms_c
    end interface

    integer(c_int) :: itone(JTTY_MAX_TONES), nsym

    itone = 0
    nsym = 0
    call genjtty_atoms_c(atoms, natoms, itone, nsym, status)
    nsym_out = 0
    if (nsym <= 0) return
    nsym_out = JTTY_ERR_ARGS
    if (max_tones < nsym) return
    itone_out(1:nsym) = itone(1:nsym)
    nsym_out = nsym
  end function jtty_encode_atoms

  !-------------------------------------------------------------------------
  ! jtty_pack_text : message text -> one 34-bit source word per frame.
  !
  !   The words are the frames jtty_encode_text sends, before the CRC and the
  !   code: source bit 1 is the most significant of the 34, bit 33 is reserved
  !   (zero) and bit 34 is end-of-message.
  !
  !   msg, msg_len, profile, sent, sent_cap : as jtty_encode_text
  !   is_final   : 1 sets end-of-message on the last frame, as a whole message
  !                does; 0 leaves it clear, for text sent while it is still
  !                being typed. Anything else is refused.
  !   frames_out : caller buffer, capacity max_frames (JTTY_MAX_FRAMES fits)
  !   returns    : frames, 1..16; 0 for an empty message; JTTY_ERR_ARGS
  !                (nothing written) or JTTY_ERR_ENCODE, as jtty_encode_text
  !-------------------------------------------------------------------------
  function jtty_pack_text(msg, msg_len, profile, is_final, frames_out, max_frames, sent, sent_cap) &
       result(nframes_out) bind(C, name="jtty_pack_text")
    use jtty_mod, only: pack_jtty
    character(kind=c_char), intent(in)  :: msg(*)
    integer(c_int), value,  intent(in)  :: msg_len, profile, is_final, max_frames, sent_cap
    integer(c_int64_t),     intent(out) :: frames_out(*)
    character(kind=c_char), intent(out) :: sent(*)
    integer(c_int) :: nframes_out

    character(len=JTTY_TEXT_LEN) :: umsg
    character(len=34) :: c32(JTTY_MAX_FRAMES)
    integer :: nframes, final, i, j, n
    integer(c_int64_t) :: word

    nframes_out = JTTY_ERR_ARGS
    if (msg_len < 0 .or. msg_len > JTTY_TEXT_LEN) return
    if (sent_cap < JTTY_TEXT_LEN + 1) return
    if (is_final /= 0 .and. is_final /= 1) return

    umsg = ' '
    do i = 1, msg_len
       if (msg(i) == c_null_char) exit
       umsg(i:i) = msg(i)
    end do

    final = is_final
    nframes = 0
    call pack_jtty(umsg, c32, nframes, int(profile), is_final=final)
    if (nframes > 0 .and. max_frames < nframes) return

    n = len_trim(umsg)
    do i = 1, n
       sent(i) = umsg(i:i)
    end do
    sent(n + 1) = c_null_char

    if (nframes <= 0) then
       nframes_out = 0
       if (n > 0) nframes_out = JTTY_ERR_ENCODE
       return
    end if
    do i = 1, nframes
       word = 0
       do j = 1, 34
          word = 2 * word
          if (c32(i)(j:j) == '1') word = word + 1
       end do
       frames_out(i) = word
    end do
    nframes_out = nframes
  end function jtty_pack_text

  !-------------------------------------------------------------------------
  ! jtty_unpack_frames : 34-bit source words -> the message text.
  !
  !   Each word is checked as the decoder checks a frame after its CRC
  !   (reserved bits, assigned forms, field ranges); a word that fails is left
  !   out of the text, as the receiver drops it.
  !
  !   frames, nframes : 1..16 words, each below 2**34, bit 1 most significant
  !   text_out        : caller buffer, capacity text_cap >= JTTY_TEXT_LEN+1;
  !                     the text as the receiver shows it, NUL-terminated
  !   eom             : 1 when every word is valid and the last carries
  !                     end-of-message, else 0
  !   returns         : 0 when every word is valid, 1 when one or more are not,
  !                     JTTY_ERR_ARGS for a refused count, word or capacity
  !                     (nothing written)
  !-------------------------------------------------------------------------
  function jtty_unpack_frames(frames, nframes, text_out, text_cap, eom) &
       result(status) bind(C, name="jtty_unpack_frames")
    use jtty_mod, only: unpack_jtty
    use jtty_mdec, only: display_message_text
    integer(c_int64_t),     intent(in)  :: frames(*)
    integer(c_int), value,  intent(in)  :: nframes, text_cap
    character(kind=c_char), intent(out) :: text_out(*)
    integer(c_int),         intent(out) :: eom
    integer(c_int) :: status

    character(len=34) :: c32(JTTY_MAX_FRAMES)
    character(len=JTTY_TEXT_LEN) :: message
    logical :: last, valid
    integer :: i, j, n

    status = JTTY_ERR_ARGS
    if (nframes < 1 .or. nframes > JTTY_MAX_FRAMES) return
    if (text_cap < JTTY_TEXT_LEN + 1) return
    if (any(frames(1:nframes) < 0) .or. any(frames(1:nframes) >= 2_c_int64_t**34)) return

    c32 = ' '
    do i = 1, nframes
       do j = 1, 34
          c32(i)(j:j) = '0'
          if (btest(frames(i), 34 - j)) c32(i)(j:j) = '1'
       end do
    end do

    call unpack_jtty(c32, int(nframes), message, is_last_frame=last, source_valid=valid)
    message = display_message_text(message)
    n = len_trim(message)
    do i = 1, n
       text_out(i) = message(i:i)
    end do
    text_out(n + 1) = c_null_char
    eom = 0
    if (last) eom = 1
    status = 0
    if (.not. valid) status = 1
  end function jtty_unpack_frames

  !-------------------------------------------------------------------------
  ! jtty_gen_wave : channel tones -> real audio at 12 kHz, amplitude 1.
  !
  !   itone     : nsym tones from jtty_encode_text/_atoms
  !   nsym      : a whole number of frames, 59..JTTY_MAX_TONES
  !   f0        : audio frequency of tone 0, Hz; the signal spans about
  !               f0 .. f0 + 4*31.25 Hz
  !   wave_out  : caller buffer, capacity nwave_cap >= nsym*JTTY_NSPS
  !   returns   : nsym*JTTY_NSPS samples, or JTTY_ERR_ARGS (nothing written)
  !
  ! gen_jttywave shapes the first and last nsps/8 samples itself; the output is
  ! exactly nsym symbols long, with no lead-in or tail.
  !-------------------------------------------------------------------------
  function jtty_gen_wave(itone, nsym, f0, wave_out, nwave_cap) &
       result(nwave_out) bind(C, name="jtty_gen_wave")
    integer(c_int),        intent(in)    :: itone(*)
    integer(c_int), value, intent(in)    :: nsym, nwave_cap
    real(c_float),  value, intent(in)    :: f0
    real(c_float),         intent(inout) :: wave_out(*)
    integer(c_int) :: nwave_out

    integer :: it(JTTY_MAX_TONES), ns, nsps, icmplx, nwave
    real    :: bt, fsample, freq
    complex :: cwave(1)   ! not referenced with icmplx=0

    nwave_out = JTTY_ERR_ARGS
    if (nsym < JTTY_FRAME_SYMBOLS .or. nsym > JTTY_MAX_TONES) return
    if (mod(nsym, JTTY_FRAME_SYMBOLS) /= 0) return
    if (any(itone(1:nsym) < 0) .or. any(itone(1:nsym) > 3)) return
    if (nwave_cap < nsym * JTTY_NSPS) return

    it(1:nsym) = itone(1:nsym)
    ns = nsym
    nsps = JTTY_NSPS
    bt = 2.0
    fsample = 12000.0
    freq = f0
    icmplx = 0
    nwave = nsym * JTTY_NSPS
    call gen_jttywave(it, ns, nsps, bt, fsample, freq, cwave, wave_out, icmplx, nwave)
    nwave_out = nwave
  end function jtty_gen_wave

  !-------------------------------------------------------------------------
  ! jtty_take_updates : drain a receive handle's pending updates into records.
  !
  !   handle  : from jtty_rx_create
  !   out     : caller array of jtty_update_t, capacity max_out
  !   returns : records written (0 when none are pending, the handle is not
  !             valid, or max_out <= 0). Each call removes what it returns;
  !             call again while it fills the array.
  !-------------------------------------------------------------------------
  function jtty_take_updates(handle, out, max_out) result(n) bind(C, name="jtty_take_updates")
    use jtty_receive_context, only: jtty_rx_take_updates
    integer(c_int), value,  intent(in)  :: handle, max_out
    type(jtty_update_t),    intent(out) :: out(*)
    integer(c_int) :: n

    character(kind=c_char) :: text(JTTY_TEXT_LEN * JTTY_TAKE_BATCH)
    integer(c_int64_t)     :: ids(JTTY_TAKE_BATCH)
    real(c_float)          :: freqs(JTTY_TAKE_BATCH)
    real(c_double)         :: starts(JTTY_TAKE_BATCH), latest(JTTY_TAKE_BATCH)
    integer(c_int)         :: terminals(JTTY_TAKE_BATCH), snrs(JTTY_TAKE_BATCH)
    integer(c_int)         :: want, got
    integer                :: i, j, last

    n = 0
    do while (n < max_out)
       want = min(max_out - n, JTTY_TAKE_BATCH)
       got = jtty_rx_take_updates(handle, want, text, ids, freqs, starts, latest, &
            terminals, snrs)
       do i = 1, got
          out(n + i)%message_id = ids(i)
          out(n + i)%start_s    = starts(i)
          out(n + i)%latest_s   = latest(i)
          out(n + i)%freq       = freqs(i)
          out(n + i)%terminal   = terminals(i)
          out(n + i)%snr        = snrs(i)
          out(n + i)%text       = c_null_char
          last = 0
          do j = 1, JTTY_TEXT_LEN
             if (text((i - 1) * JTTY_TEXT_LEN + j) /= ' ') last = j
          end do
          do j = 1, last
             out(n + i)%text(j) = text((i - 1) * JTTY_TEXT_LEN + j)
          end do
       end do
       n = n + got
       if (got < want) exit
    end do
  end function jtty_take_updates

end module jtty_cabi
