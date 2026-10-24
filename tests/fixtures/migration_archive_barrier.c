/* Test-only Linux barrier after the owned process takes its WAL write lock or fsyncs.
 * SQLite 3.53.2: WAL_WRITE_LOCK=0, UNIX_SHM_BASE=(22+SQLITE_SHM_NLOCK)*4=120.
 * No product fault flags; exact descriptor path and existing manifest required.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int paused = 0;
static int barrier(int fd, const char *target, const char *label) {
    const char *manifest = getenv("HT_TEST_MIGRATION_MANIFEST");
    const char *ready = getenv("HT_TEST_MIGRATION_READY");
    const char *release = getenv("HT_TEST_MIGRATION_RELEASE");
    char link[64], path[PATH_MAX + 1];
    if (paused || !target || !manifest || !ready || !release) return 0;
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    if (n < 0 || (size_t)n >= sizeof(path) - 1) return 0;
    path[n] = 0;
    if (strcmp(path, target) || access(manifest, F_OK)) return 0;
    paused = 1;
    int marker = open(ready, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (marker < 0) return -1;
    size_t bytes = strlen(label);
    if (write(marker, label, bytes) != (ssize_t)bytes) { close(marker); return -1; }
    if (close(marker)) return -1;
    struct timespec start, now, tick = {0, 1000000};
    if (clock_gettime(CLOCK_MONOTONIC, &start)) return -1;
    for (;;) {
        if (access(release, F_OK) == 0) return 0;
        if (clock_gettime(CLOCK_MONOTONIC, &now)) return -1;
        if (now.tv_sec - start.tv_sec >= 5) { errno = ETIMEDOUT; return -1; }
        nanosleep(&tick, NULL);
    }
}

int fsync(int fd) {
    int (*real_fsync)(int) = (int (*)(int))dlsym(RTLD_NEXT, "fsync");
    if (!real_fsync) { errno = ENOSYS; return -1; }
    int result = real_fsync(fd);
    int saved = errno;
    if (result == 0 && barrier(fd, getenv("HT_TEST_MIGRATION_SYNC"), "synced\n")) return -1;
    errno = saved;
    return result;
}

int fcntl64(int fd, int cmd, ...) {
    int (*real_fcntl)(int, int, ...) = (int (*)(int, int, ...))dlsym(RTLD_NEXT, "fcntl64");
    if (!real_fcntl) { errno = ENOSYS; return -1; }
    va_list args;
    va_start(args, cmd);
    if (cmd == F_SETLK64 || cmd == F_SETLKW64 || cmd == F_GETLK64) {
        struct flock64 *lock = va_arg(args, struct flock64 *);
        va_end(args);
        int result = real_fcntl(fd, cmd, lock);
        int saved = errno;
        if (result == 0 && (cmd == F_SETLK64 || cmd == F_SETLKW64) &&
            lock->l_type == F_WRLCK && lock->l_start == 120 && lock->l_len == 1 &&
            barrier(fd, getenv("HT_TEST_MIGRATION_SHM"), "locked\n")) return -1;
        errno = saved;
        return result;
    }
    switch (cmd) {
        case F_GETFD: case F_GETFL: case F_GETOWN: case F_GETSIG:
        case F_GETLEASE: case F_GETPIPE_SZ: case F_GET_SEALS:
            va_end(args);
            return real_fcntl(fd, cmd);
        case F_DUPFD: case F_DUPFD_CLOEXEC: case F_SETFD: case F_SETFL:
        case F_SETOWN: case F_SETSIG: case F_SETLEASE: case F_SETPIPE_SZ:
        case F_ADD_SEALS: {
            int value = va_arg(args, int);
            va_end(args);
            return real_fcntl(fd, cmd, value);
        }
        default:
            va_end(args);
            errno = ENOSYS; /* unexpected fixture syscall must not appear successful */
            return -1;
    }
}
