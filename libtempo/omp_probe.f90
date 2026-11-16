! Nexus: OpenMP runtime self-check for libtempo.
!
! The multithreaded FT8 decoder (WSJT-X's lib/ft8var) runs on OpenMP, so libtempo links the GNU
! OpenMP runtime (libgomp) on every platform it ships on. This probe is what proves that link
! is real and works where it runs: it opens a parallel region of `nreq` threads, and every
! thread works through a 4 MB automatic array. Compiled with -fopenmp, gfortran puts that array
! on the THREAD'S stack (-fopenmp implies -frecursive), so a worker whose stack is smaller than
! WSJT-X's OMP_STACKSIZE of 10M crashes here instead of in the middle of a decode. See
! omp_env.c for how libtempo sets that stack size before libgomp starts.
!
!   tempo_omp_probe(nreq, nran, nmax)
!     nreq  in   threads to request (>= 1)
!     nran  out  threads that ran the region and got the right answer
!     nmax  out  omp_get_max_threads(), the runtime's own default team size

subroutine tempo_omp_probe(nreq, nran, nmax) bind(C, name='tempo_omp_probe')
  use iso_c_binding, only: c_int
  !$ use omp_lib
  implicit none
  integer(c_int), value, intent(in) :: nreq
  integer(c_int), intent(out) :: nran, nmax
  integer :: hits
  interface
     integer(c_int) function tempo_omp_env_linked() bind(C, name='tempo_omp_env_linked')
       import :: c_int
     end function tempo_omp_env_linked
  end interface

  ! Keeps omp_env.c's stack-size constructor in the link (see omp_env.c).
  nran = tempo_omp_env_linked()
  nmax = 1
  !$ nmax = omp_get_max_threads()
  hits = 0
  !$omp parallel num_threads(max(1, nreq)) reduction(+:hits)
  call omp_probe_worker(hits)
  !$omp end parallel
  nran = hits
end subroutine tempo_omp_probe

subroutine omp_probe_worker(hits)
  implicit none
  integer, intent(inout) :: hits
  integer, parameter :: N = 1000000      ! 4 MB of real: over every platform's default
  real :: big(N)                         ! secondary-thread stack (macOS 512 KB, Linux 2-8 MB)
  integer :: i
  real(kind=8) :: s

  do i = 1, N
     big(i) = real(mod(i, 7))
  enddo
  s = 0.d0
  do i = N, 1, -1
     s = s + big(i)
  enddo
  ! sum of mod(i,7) over 1..1e6 = 142857 full cycles of 21, plus the remainder 1 -> 2999998
  if (nint(s) .eq. 2999998) hits = hits + 1
end subroutine omp_probe_worker
