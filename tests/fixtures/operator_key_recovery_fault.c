/* Test-only exact-child key recovery faults; never log secret bytes or digests. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int record(const char *phase, const char *outcome) {
    const char *name = getenv("HT_TEST_KEY_LOG");
    if (!name) return -1;
    int fd = open(name, O_WRONLY | O_APPEND | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return -1;
    char line[64];
    int n = snprintf(line, sizeof(line), "%s:%s\n", phase, outcome);
    if (n <= 0 || (size_t)n >= sizeof(line)) { close(fd); return -1; }
    size_t left = (size_t)n;
    const char *next = line;
    while (left) {
        ssize_t wrote = write(fd, next, left);
        if (wrote <= 0) { close(fd); return -1; }
        left -= (size_t)wrote;
        next += wrote;
    }
    return close(fd);
}

static int signal_and_wait(const char *phase) {
    const char *ready = getenv("HT_TEST_KEY_READY");
    const char *release = getenv("HT_TEST_KEY_RELEASE");
    if (!ready || !release) return -1;
    int fd = open(ready, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (fd < 0) return -1;
    size_t size = strlen(phase);
    ssize_t wrote = write(fd, phase, size);
    if (close(fd) || wrote != (ssize_t)size) return -1;
    for (int i = 0; i < 500; ++i) {
        if (access(release, F_OK) == 0) return 0;
        usleep(10000);
    }
    return -1;
}

int fsync(int fd) {
    int (*real_fsync)(int) = (int (*)(int))dlsym(RTLD_NEXT, "fsync");
    if (!real_fsync) { errno = ENOSYS; return -1; }
    const char *state = getenv("HT_TEST_KEY_STATE");
    const char *parent = getenv("HT_TEST_KEY_PARENT");
    const char *staged = getenv("HT_TEST_KEY_STAGED");
    const char *final = getenv("HT_TEST_KEY_FINAL");
    const char *mode = getenv("HT_TEST_KEY_MODE");
    if (!state || !parent || !staged || !final || !mode) return real_fsync(fd);
    char link[64], path[PATH_MAX + 1];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    if (n < 0 || (size_t)n >= sizeof(path) - 1) return real_fsync(fd);
    path[n] = 0;
    const char *phase = NULL;
    if (!strcmp(path, parent)) phase = "entry";
    else if (!strcmp(path, staged) || !strcmp(path, final)) phase = "file";
    else if (!strcmp(path, state)) phase = access(staged, F_OK) ? "post" : "pre";
    if (!phase) return real_fsync(fd);
    char selected[32] = {0};
    int m = open(mode, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    ssize_t got = m < 0 ? -1 : read(m, selected, sizeof(selected) - 1);
    if (m >= 0) close(m);
    if (got <= 0 || !strcmp(selected, phase)) {
        (void)record(phase, "fail"); errno = EIO; return -1;
    }
    if ((!strcmp(selected, "mutate-bytes") || !strcmp(selected, "mutate-inode")) && !strcmp(phase, "file")) {
        const char *retained = getenv("HT_TEST_KEY_RETAINED");
        if (!retained) { errno = EINVAL; return -1; }
        char value[64];
        if (pread(fd, value, sizeof(value), 0) != (ssize_t)sizeof(value)) { errno = EIO; return -1; }
        if (!strcmp(selected, "mutate-inode")) {
            if (rename(path, retained)) return -1;
        } else {
            value[0] = value[0] == '0' ? '1' : '0';
        }
        int flags = O_WRONLY | O_CLOEXEC | O_NOFOLLOW;
        if (!strcmp(selected, "mutate-inode")) flags |= O_CREAT | O_EXCL;
        int target = open(path, flags, 0600);
        if (target < 0) return -1;
        ssize_t wrote = write(target, value, sizeof(value));
        if (close(target) || wrote != (ssize_t)sizeof(value)) { errno = EIO; return -1; }
        if (record(phase, "mutated")) { errno = EIO; return -1; }
        return real_fsync(fd);
    }
    if (!strcmp(selected, "race-final") && !strcmp(phase, "pre")) {
        int target = open(final, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
        if (target < 0) return -1;
        const char text[] = "racing-final-preserve";
        ssize_t wrote = write(target, text, sizeof(text) - 1);
        if (close(target) || wrote != (ssize_t)(sizeof(text) - 1)) { errno = EIO; return -1; }
        if (record(phase, "raced")) { errno = EIO; return -1; }
        return real_fsync(fd);
    }
    char hold[32];
    snprintf(hold, sizeof(hold), "pause-%s", phase);
    if (!strcmp(selected, hold)) {
        if (record(phase, "paused") || signal_and_wait(phase)) { errno = EIO; return -1; }
    }
    int result = real_fsync(fd);
    int saved = errno;
    if (record(phase, result ? "error" : "ok")) { errno = EIO; return -1; }
    errno = saved;
    return result;
}
