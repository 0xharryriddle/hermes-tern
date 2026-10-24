/* Test-only Linux fsync interposer. Match one exact fixture directory only. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int record(const char *text) {
    const char *path = getenv("HT_TEST_FSYNC_LOG");
    if (!path) return -1;
    int fd = open(path, O_WRONLY | O_APPEND | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return -1;
    size_t left = strlen(text);
    while (left) {
        ssize_t n = write(fd, text, left);
        if (n <= 0) { close(fd); return -1; }
        text += n;
        left -= (size_t)n;
    }
    return close(fd);
}

int fsync(int fd) {
    int (*real_fsync)(int) = (int (*)(int))dlsym(RTLD_NEXT, "fsync");
    if (!real_fsync) { errno = ENOSYS; return -1; }
    const char *target = getenv("HT_TEST_FSYNC_TARGET");
    char link[64], path[PATH_MAX + 1];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    if (!target || n < 0 || (size_t)n >= sizeof(path) - 1) return real_fsync(fd);
    path[n] = 0;
    if (strcmp(path, target) != 0) return real_fsync(fd);

    const char *mode_path = getenv("HT_TEST_FSYNC_MODE");
    int mode = mode_path ? open(mode_path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW) : -1;
    char enabled = 0;
    ssize_t read_count = mode < 0 ? -1 : read(mode, &enabled, 1);
    if (mode >= 0) close(mode);
    if (read_count != 1 || enabled != '0') {
        (void)record("fail\n");
        errno = EIO;
        return -1;
    }
    int result = real_fsync(fd);
    int saved_errno = errno;
    if (record(result == 0 ? "ok\n" : "error\n") != 0) { errno = EIO; return -1; }
    errno = saved_errno;
    return result;
}
