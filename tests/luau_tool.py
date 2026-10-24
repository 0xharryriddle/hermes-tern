"""Pinned private test interpreter. Never install plugins or change PATH/profiles."""
import hashlib
import io
import os
from pathlib import Path
import stat
import urllib.request
import zipfile

ARCHIVE_SHA256 = "99d45afb4afaee00ba631c526c8e8c76c67b60f6f119953ce6409be6d6953663"
BINARY_SHA256 = "416fec2184d0bd238be24721f0369ec1d38b09da21032d6845e5f6648f69f2f8"
URL = "https://github.com/luau-lang/luau/releases/download/0.743/luau-ubuntu.zip"


def verified(path):
    p = Path(path)
    st = p.lstat()
    if not stat.S_ISREG(st.st_mode) or st.st_uid != os.geteuid() or st.st_nlink != 1 or st.st_mode & 0o077:
        raise RuntimeError("unsafe Luau test tool")
    if st.st_size != 3170056 or hashlib.sha256(p.read_bytes()).hexdigest() != BINARY_SHA256:
        raise RuntimeError("Luau test tool differs from pinned release")
    return p


def interpreter():
    explicit = os.environ.get("HERMES_TERN_TEST_LUAU")
    if explicit:
        if not Path(explicit).is_absolute():
            raise RuntimeError("Luau test tool must be absolute")
        return verified(explicit)
    base = Path(os.environ.get("XDG_CACHE_HOME", str(Path.home() / ".cache"))) / "hermes-tern" / "test-tools"
    base.mkdir(mode=0o700, parents=True, exist_ok=True)
    st = base.lstat()
    if not stat.S_ISDIR(st.st_mode) or st.st_uid != os.geteuid() or st.st_mode & 0o077:
        raise RuntimeError("unsafe test-tool cache")
    tool = base / ("luau-0.743-" + BINARY_SHA256)
    if tool.exists() or tool.is_symlink():
        return verified(tool)
    request = urllib.request.Request(URL, headers={"User-Agent": "hermes-tern-test-tool"})
    with urllib.request.urlopen(request, timeout=30) as response:
        archive = response.read(7733850)
    if len(archive) != 7733849 or hashlib.sha256(archive).hexdigest() != ARCHIVE_SHA256:
        raise RuntimeError("Luau archive differs from pinned release")
    with zipfile.ZipFile(io.BytesIO(archive)) as package:
        item = package.getinfo("luau")
        if item.file_size != 3170056:
            raise RuntimeError("Luau member exceeds pinned bound")
        raw = package.read(item)
    if hashlib.sha256(raw).hexdigest() != BINARY_SHA256:
        raise RuntimeError("Luau member digest mismatch")
    # A partial/racing existing file is never silently removed or replaced.
    try:
        fd = os.open(tool, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o700)
    except FileExistsError:
        return verified(tool)
    with os.fdopen(fd, "wb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    return verified(tool)
