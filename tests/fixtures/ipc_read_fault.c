/* Test-only Linux socket-read fault, scoped to one exact abstract IPC endpoint. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

static int injected;
static int prefixed;

static int matches(int fd) {
    const char *target = getenv("HT_TEST_IPC_TARGET");
    struct sockaddr_un address;
    socklen_t size = sizeof(address);
    memset(&address, 0, sizeof(address));
    if (!target || getpeername(fd, (struct sockaddr *)&address, &size) != 0 ||
        address.sun_family != AF_UNIX || address.sun_path[0] != 0) return 0;
    size_t offset = offsetof(struct sockaddr_un, sun_path) + 1;
    return size >= offset && (size_t)size - offset == strlen(target) &&
           memcmp(address.sun_path + 1, target, strlen(target)) == 0;
}

static void record(void) {
    const char *path = getenv("HT_TEST_IPC_LOG");
    if (!path) return;
    int fd = open(path, O_WRONLY | O_APPEND | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return;
    (void)write(fd, "injected\n", 9);
    (void)close(fd);
}

static int fault(int fd, size_t *length) {
    const char *mode = getenv("HT_TEST_IPC_FAULT");
    if (!mode || (injected && strcmp(mode, "always-eintr") != 0) || !matches(fd)) return 0;
    if (strcmp(mode, "eintr-after-prefix") == 0 && !prefixed) {
        prefixed = 1;
        if (*length > 8) *length = 8;
        return 0;
    }
    if (!injected) record();
    injected = 1;
    errno = strcmp(mode, "reset") == 0 ? ECONNRESET : EINTR;
    return 1;
}

ssize_t recv(int fd, void *buffer, size_t length, int flags) {
    ssize_t (*next)(int, void *, size_t, int) = dlsym(RTLD_NEXT, "recv");
    if (!next) { errno = ENOSYS; return -1; }
    if (fault(fd, &length)) return -1;
    return next(fd, buffer, length, flags);
}

ssize_t recvfrom(int fd, void *buffer, size_t length, int flags,
                 struct sockaddr *address, socklen_t *address_length) {
    ssize_t (*next)(int, void *, size_t, int, struct sockaddr *, socklen_t *) = dlsym(RTLD_NEXT, "recvfrom");
    if (!next) { errno = ENOSYS; return -1; }
    if (fault(fd, &length)) return -1;
    return next(fd, buffer, length, flags, address, address_length);
}

ssize_t read(int fd, void *buffer, size_t length) {
    ssize_t (*next)(int, void *, size_t) = dlsym(RTLD_NEXT, "read");
    if (!next) { errno = ENOSYS; return -1; }
    if (fault(fd, &length)) return -1;
    return next(fd, buffer, length);
}
