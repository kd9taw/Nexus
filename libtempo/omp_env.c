/*
 * Nexus: give OpenMP worker threads the stack WSJT-X gives them, before libgomp starts.
 *
 * WSJT-X starts its decoder with OMP_STACKSIZE=10M (widgets/mainwindow.cpp: the jt9 process
 * environment). Its multithreaded FT8 decoder (lib/ft8var) is compiled with -fopenmp, which puts
 * every local array on the stack, and some are megabytes. A worker thread gets the platform's
 * default stack unless OMP_STACKSIZE says otherwise (512 KB on macOS, 1-2 MB on Windows), which
 * is too small. Nexus runs the decoder in its own process, so it sets the same value itself.
 *
 * libgomp reads its environment ONCE, in a load-time constructor, so setting it from Rust's main
 * is too late. This constructor runs earlier: libgomp is linked statically, into the same image,
 * after libtempo, and a constructor with priority 101 runs before default-priority ones (ELF and
 * PE); on Mach-O, which has no priorities, initializers run in link order, and libtempo is linked
 * before libgomp. An OMP_STACKSIZE the operator set is left alone. omp_probe.f90 proves the result
 * on every platform: its workers each need 4 MB of stack.
 *
 * A static archive member is linked only when something references it, and a constructor alone
 * references nothing, so every libtempo entry that runs OpenMP calls tempo_omp_env_linked(): that
 * call is what pulls this object, and its constructor, into the binary.
 */
#include <stdlib.h>

#if defined(__APPLE__)
__attribute__((constructor))
#else
__attribute__((constructor(101)))
#endif
static void tempo_omp_stacksize(void)
{
    if (getenv("OMP_STACKSIZE") != NULL)
        return;
#if defined(_WIN32)
    _putenv("OMP_STACKSIZE=10M");
#else
    setenv("OMP_STACKSIZE", "10M", 0);
#endif
}

/* Referenced by every OpenMP entry point so the linker keeps this object; see above. */
int tempo_omp_env_linked(void)
{
    return 1;
}
