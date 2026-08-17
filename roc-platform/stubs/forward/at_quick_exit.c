/* See atexit.c. glibc keeps at_quick_exit in libc_nonshared.a too. */

extern int __cxa_at_quick_exit(void (*func)(void *), void *dso_handle);
extern void *__dso_handle;

int at_quick_exit(void (*func)(void)) {
    return __cxa_at_quick_exit((void (*)(void *))func, __dso_handle);
}
