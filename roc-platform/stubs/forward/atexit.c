/* glibc defines atexit only in libc_nonshared.a. libc.so.6 exports no atexit
 * dynamic symbol on any version, so a stub shared object cannot supply it.
 *
 * No glibc headers: stdlib.h applies __REDIRECT renames and would conflict
 * with this definition.
 *
 * The function-pointer cast matches glibc's own stdlib/atexit.c. */

extern int __cxa_atexit(void (*func)(void *), void *arg, void *dso_handle);
extern void *__dso_handle;

int atexit(void (*func)(void)) {
    return __cxa_atexit((void (*)(void *))func, 0, __dso_handle);
}
