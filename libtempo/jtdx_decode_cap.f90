! Nexus: the decode-list bound of the JTDX-style FT8 and FT4 decoders.
!
! Upstream (WSJT-X 3.0.2 lib/ft8var, JTDX 2.2.159 lib/ft4_decode.f90) appends every new decode of
! a period to a fixed list, ft8_mod1's allmessages(200) and ft4_decode's decodes(100), with no
! check against its size: one decode past it writes over whatever lies beyond the array. The
! decoders (MODIFIED FOR NEXUS there) stop adding once their list is full, and a period then
! reports the first 200 (FT8) or 100 (FT4) decodes it found.
!
! The bound is the list's size. ncapft8/ncapft4 can only lower it, and nothing in Nexus does, so a
! shipped decoder always stops at the size; they exist so a test can drive a real period to the
! bound, which no natural band reaches (jtdx_set_decode_caps, below).
module jtdx_decode_cap
  use iso_c_binding, only: c_int
  implicit none
  integer :: ncapft8 = huge(0)
  integer :: ncapft4 = huge(0)

contains

  !-------------------------------------------------------------------------
  ! jtdx_set_decode_caps : FOR TESTS. Lower the FT8 and FT4 decode-list bound to nft8 / nft4 (a
  ! value <= 0 restores the list's size). Set it before a decode, never during one.
  !-------------------------------------------------------------------------
  subroutine jtdx_set_decode_caps(nft8, nft4) bind(C, name="jtdx_set_decode_caps")
    integer(c_int), value, intent(in) :: nft8, nft4
    ncapft8 = huge(0)
    ncapft4 = huge(0)
    if (nft8 > 0) ncapft8 = nft8
    if (nft4 > 0) ncapft4 = nft4
  end subroutine jtdx_set_decode_caps

end module jtdx_decode_cap
