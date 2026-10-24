/* Test-only Linux fsync faults for exact owned credential recovery boundaries. */
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
    const char *path = getenv("HT_TEST_CRED_LOG");
    if (!path) return -1;
    int fd = open(path, O_WRONLY | O_APPEND | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return -1;
    char line[64];
    int length = snprintf(line, sizeof(line), "%s:%s\n", phase, outcome);
    if (length <= 0 || (size_t)length >= sizeof(line)) { close(fd); return -1; }
    size_t left = (size_t)length;
    const char *next = line;
    while (left) {
        ssize_t n = write(fd, next, left);
        if (n <= 0) { close(fd); return -1; }
        next += n;
        left -= (size_t)n;
    }
    return close(fd);
}

int fsync(int fd) {
    int (*real_fsync)(int) = (int (*)(int))dlsym(RTLD_NEXT, "fsync");
    if (!real_fsync) { errno = ENOSYS; return -1; }
    const char *arm = getenv("HT_TEST_CRED_ARM");
    const char *state = getenv("HT_TEST_CRED_STATE");
    const char *dir = getenv("HT_TEST_CRED_DIR");
    const char *staged = getenv("HT_TEST_CRED_STAGED");
    const char *final = getenv("HT_TEST_CRED_FINAL");
    const char *mode = getenv("HT_TEST_CRED_MODE");
    char link[64], path[PATH_MAX + 1];
    if (!arm || !state || !dir || !staged || !final || !mode || access(arm, F_OK)) return real_fsync(fd);
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    if (n < 0 || (size_t)n >= sizeof(path) - 1) return real_fsync(fd);
    path[n] = 0;
    const char *phase = NULL;
    if (!strcmp(path, state)) phase = "entry";
    else if (!strcmp(path, staged) || !strcmp(path, final)) phase = "file";
    else if (!strcmp(path, dir)) phase = access(staged, F_OK) ? "post" : "pre";
    if (!phase) return real_fsync(fd);
    char selected[16] = {0};
    int modefd = open(mode, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    ssize_t got = modefd < 0 ? -1 : read(modefd, selected, sizeof(selected) - 1);
    if (modefd >= 0) close(modefd);
    if (got <= 0 || !strcmp(selected, phase)) {
        (void)record(phase, "fail");
        errno = EIO;
        return -1;
    }
    int result = real_fsync(fd);
    int saved = errno;
    if (record(phase, result ? "error" : "ok")) { errno = EIO; return -1; }
    errno = saved;
    return result;
}
