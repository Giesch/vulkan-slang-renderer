/* See atexit.c. glibc keeps pthread_atfork in libc_nonshared.a too.
 *
 * __register_atfork is exported only under GLIBC_PRIVATE. An unversioned
 * reference binds to the default version, but GLIBC_PRIVATE carries no
 * stability promise. generate.sh warns about that. */

extern int __register_atfork(void (*prepare)(void), void (*parent)(void),
                             void (*child)(void), void *dso_handle);
extern void *__dso_handle;

int pthread_atfork(void (*prepare)(void), void (*parent)(void),
                   void (*child)(void)) {
    return __register_atfork(prepare, parent, child, __dso_handle);
}
