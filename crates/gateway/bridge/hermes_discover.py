"""Private pipe helper. Never starts, recovers or restarts a Hermes owner."""
import contextlib
import importlib.util
import json
import os
from pathlib import Path
import sys


def main():
    try:
        root = Path(sys.argv[1])
        if not root.is_absolute():
            raise ValueError("root")
        os.environ["HERMES_PYTHON_SRC_ROOT"] = str(root)
        if len(sys.argv) == 3:
            home = Path(sys.argv[2])
            if not home.is_absolute():
                raise ValueError("home")
            os.environ["HERMES_HOME"] = str(home)
        # Upstream imports may print diagnostics containing secrets. Neither stream
        # is a diagnostic channel during import/discovery; only the grant escapes.
        with open(os.devnull, "w") as sink:
            with contextlib.redirect_stdout(sink), contextlib.redirect_stderr(sink):
                spec = importlib.util.spec_from_file_location(
                    "hermes_tern_gateway_bootstrap", root / "ui-tui/scripts/gateway_bootstrap.py")
                if spec is None or spec.loader is None:
                    raise ImportError("bootstrap module unavailable")
                module = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(module)
                grant = module.bootstrap(False, False)
        if not isinstance(grant, dict) or set(grant) != {"url", "protocols", "profile_id", "instance_id"}:
            raise ValueError("grant")
        encoded = json.dumps(grant, separators=(",", ":"))
        if len(encoded.encode()) > 65536:
            raise ValueError("size")
        # Rust validates the URL, identities and secret subprotocol before use.
        print(encoded)
        return 0
    except BaseException:
        print("gateway unavailable: discovery_failed", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
