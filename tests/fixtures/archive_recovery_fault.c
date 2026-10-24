/* Test-only Linux fsync faults: one owned archive root and canonical staging children. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int record(const char *kind, const char *outcome) {
    const char *path = getenv("HT_TEST_RECOVERY_LOG");
    if (!path) return -1;
    int fd = open(path, O_WRONLY | O_APPEND | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return -1;
    char text[80];
    int count = snprintf(text, sizeof(text), "%s:%s\n", kind, outcome);
    if (count <= 0 || count >= (int)sizeof(text)) { close(fd); return -1; }
    size_t left = (size_t)count;
    const char *p = text;
    while (left) {
        ssize_t n = write(fd, p, left);
        if (n <= 0) { close(fd); return -1; }
        p += n;
        left -= (size_t)n;
    }
    return close(fd);
}
static int uuid(const char *s) {
    for (int i = 0; i < 36; i++) {
        if (!s[i]) return 0;
        if (i == 8 || i == 13 || i == 18 || i == 23) { if (s[i] != '-') return 0; }
        else if (!((s[i] >= '0' && s[i] <= '9') || (s[i] >= 'a' && s[i] <= 'f'))) return 0;
    }
    return 1;
}
int fsync(int fd) {
    int (*real_fsync)(int) = (int (*)(int))dlsym(RTLD_NEXT, "fsync");
    if (!real_fsync) { errno = ENOSYS; return -1; }
    const char *root = getenv("HT_TEST_RECOVERY_ROOT");
    char link[64], path[PATH_MAX + 1];
    snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
    ssize_t n = readlink(link, path, sizeof(path) - 1);
    if (!root || n < 0 || (size_t)n >= sizeof(path) - 1) return real_fsync(fd);
    path[n] = 0;
    const char *kind = NULL;
    if (!strcmp(path, root)) kind = "root";
    else {
        size_t size = strlen(root);
        if (strncmp(path, root, size) || path[size] != '/') return real_fsync(fd);
        const char *p = path + size + 1;
        if (!strncmp(p, ".pending-", 9)) p += 9;
        if (!uuid(p)) return real_fsync(fd);
        p += 36;
        if (!*p) {
            /* SQLite also syncs its output parent before our complete bundle exists. */
            char manifest[PATH_MAX + 1];
            int length = snprintf(manifest, sizeof(manifest), "%s/manifest.json", path);
            if (length < 0 || length >= (int)sizeof(manifest) || access(manifest, F_OK)) return real_fsync(fd);
            kind = "directory";
        }
        else if (!strcmp(p, "/coordination.sqlite3")) kind = "database";
        else if (!strcmp(p, "/manifest.json")) kind = "manifest";
        else return real_fsync(fd);
    }
    const char *mode_path = getenv("HT_TEST_RECOVERY_MODE");
    char mode[32] = {0};
    int control = mode_path ? open(mode_path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW) : -1;
    ssize_t size = control < 0 ? -1 : read(control, mode, sizeof(mode) - 1);
    if (control >= 0) close(control);
    if (size <= 0 || !strcmp(mode, kind)) {
        (void)record(kind, "fail"); errno = EIO; return -1;
    }
    int result = real_fsync(fd);
    int saved = errno;
    if (!strcmp(mode, "race") && !strcmp(kind, "directory")) {
        const char *target = getenv("HT_TEST_RECOVERY_FINAL");
        if (!target || mkdir(target, 0700)) { errno = EIO; return -1; }
        char winner[PATH_MAX + 1];
        int length = snprintf(winner, sizeof(winner), "%s/winner", target);
        if (length < 0 || length >= (int)sizeof(winner)) { errno = EIO; return -1; }
        int f = open(winner, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
        if (f < 0) return -1;
        if (write(f, "winner", 6) != 6) { close(f); return -1; }
        if (close(f) || record(kind, "race")) { errno = EIO; return -1; }
    }
    if (record(kind, result == 0 ? "ok" : "error")) { errno = EIO; return -1; }
    errno = saved;
    return result;
}
