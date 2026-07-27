subroutine bpdecode40(llr,maxiterations,decoded,niterations)
!
! A log-domain belief propagation decoder for the msk40 code.
! The code is a regular (32,16) code with column weight 3, row weights 5,6,7.
! k9an August, 2016
!
integer, parameter:: N=32, K=16, M=N-K
integer*1 codeword(N),cw(N)
integer*1 colorder(N)
integer*1 decoded(K)
integer Nm(7,M)  ! 5,6 or 7 bits per check 
integer Mn(3,N)  ! 3 checks per bit
integer synd(M)
real tov(3,N)
real toc(7,M)
real tanhtoc(7,M)
real zn(N)
real llr(N)
real Tmn
integer nrw(M)

data colorder/ &
    4,   1,   2,   3,   0,   8,   6,  10, &
   13,  28,  20,  23,  17,  15,  27,  25, &
   16,  12,  18,  19,   7,  21,  22,  11, &
   24,   5,  26,  14,   9,  29,  30,  31/

data Mn/               &
  1, 6, 13,  & 
  2, 3, 14,  & 
  4, 8, 15,  & 
  5, 11, 12,  & 
  7, 10, 16,  & 
  6, 9, 15,  & 
  1, 11, 16,  & 
  2, 4, 5,  & 
  3, 7, 9,  & 
  8, 10, 12,  & 
  8, 13, 14,  & 
  1, 4, 12,  & 
  2, 6, 10,  & 
  3, 11, 15,  & 
  5, 9, 14,  & 
  7, 13, 15,  & 
  12, 14, 16,  & 
  1, 2, 8,  & 
  3, 5, 6,  & 
  4, 9, 11,  & 
  1, 7, 14,  & 
  5, 10, 13,  & 
  3, 4, 16,  & 
  2, 15, 16,  & 
  6, 7, 12,  & 
  7, 8, 11,  & 
  1, 9, 10,  & 
  2, 11, 13,  & 
  3, 12, 13,  & 
  4, 6, 14,  & 
  1, 5, 15,  & 
  8, 9, 16/

data Nm/               &
1, 7, 12, 18, 21, 27, 31,  & 
2, 8, 13, 18, 24, 28, 0, & 
2, 9, 14, 19, 23, 29, 0, & 
3, 8, 12, 20, 23, 30, 0, & 
4, 8, 15, 19, 22, 31, 0, & 
1, 6, 13, 19, 25, 30, 0, & 
5, 9, 16, 21, 25, 26, 0, & 
3, 10, 11, 18, 26, 32, 0, & 
6, 9, 15, 20, 27, 32,  0,& 
5, 10, 13, 22, 27, 0, 0, & 
4, 7, 14, 20, 26, 28, 0, & 
4, 10, 12, 17, 25, 29, 0, & 
1, 11, 16, 22, 28, 29, 0, & 
2, 11, 15, 17, 21, 30, 0, & 
3, 6, 14, 16, 24, 31, 0, & 
5, 7, 17, 23, 24, 32, 0/  

data nrw/7,6,6,6,6,6,6,6,6,5,6,6,6,6,6,6/ 

ncw=3

toc=0
tov=0
tanhtoc=0

! initialize messages to checks
do j=1,M
  do i=1,nrw(j)
    toc(i,j)=llr((Nm(i,j)))
  enddo
enddo

do iter=0,maxiterations

! Update bit log likelihood ratios (tov=0 in iteration 0).
  do i=1,N
    zn(i)=llr(i)+sum(tov(1:ncw,i))
  enddo

! Check to see if we have a codeword (check before we do any iteration).
  cw=0
  where( zn .gt. 0. ) cw=1
  ncheck=0
  do i=1,M
    synd(i)=sum(cw(Nm(1:nrw(i),i)))
    if( mod(synd(i),2) .ne. 0 ) ncheck=ncheck+1
  enddo

  if( ncheck .eq. 0 ) then ! we have a codeword - reorder the columns and return it
    niterations=iter
    codeword=cw(colorder+1)
    decoded=codeword(M+1:N)
    return
  endif

! Send messages from bits to check nodes 
  do j=1,M
    do i=1,nrw(j)
      ibj=Nm(i,j)
      toc(i,j)=zn(ibj)  
      do kk=1,ncw ! subtract off what the bit had received from the check
        if( Mn(kk,ibj) .eq. j ) then  
          toc(i,j)=toc(i,j)-tov(kk,ibj)
        endif
      enddo
    enddo
  enddo

! send messages from check nodes to variable nodes
  do i=1,M
    tanhtoc(1:7,i)=tanh(-toc(1:7,i)/2)
  enddo

  do j=1,N
    do i=1,ncw
      ichk=Mn(i,j)  ! Mn(:,j) are the checks that include bit j
      Tmn=product(tanhtoc(1:nrw(ichk),ichk),mask=Nm(1:nrw(ichk),ichk).ne.j)
      call platanh(-Tmn,y)
      tov(i,j)=2*y
    enddo
  enddo

enddo
niterations=-1
return
end subroutine bpdecode40
