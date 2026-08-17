/* See atexit.c. glibc keeps __stack_chk_fail_local in libc_nonshared.a too.
 * It is the hidden, PIC-local entry point that -fstack-protector emits calls
 * to; __stack_chk_fail is the exported one. */

extern void __stack_chk_fail(void) __attribute__((noreturn));

void __stack_chk_fail_local(void) __attribute__((noreturn));

void __stack_chk_fail_local(void) { __stack_chk_fail(); }
