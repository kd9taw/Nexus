subroutine chkcrc13a(decoded,nbadcrc)

  use crc
  integer*1 decoded(90)
  integer*1, target:: i1Dec8BitBytes(12)
  character*90 cbits

! Write decoded bits into cbits: 77-bit message plus 13-bit CRC
  write(cbits,1000) decoded
1000 format(90i1)
  read(cbits,1001) i1Dec8BitBytes
1001 format(12b8)
  read(cbits,1002) ncrc13                         !Received CRC13
1002 format(77x,b13)

  i1Dec8BitBytes(10)=iand(i1Dec8BitBytes(10),transfer(128+64+32+16+8,0_1))
  i1Dec8BitBytes(11:12)=0
  icrc13=crc13(c_loc(i1Dec8BitBytes),12)          !CRC13 computed from 77 msg bits

  nbadcrc=1
  if(ncrc13.eq.icrc13) nbadcrc=0
  
  return
end subroutine chkcrc13a
