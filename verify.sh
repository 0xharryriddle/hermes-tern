#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
export PYTHONDONTWRITEBYTECODE=1
# A caller may select a different scratch root. Never use the system temp dir.
export TMPDIR="${TMPDIR:-${XDG_CACHE_HOME:-$HOME/.cache}/hermes-tern/tmp}"
mkdir -p -- "$TMPDIR"
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --workspace --bins --locked
python3 -B -m unittest discover -s tests -v
python3 -B scripts/check_docs.py
# Check untracked first-add source too; git diff cannot see those files.
python3 -B -c 'from pathlib import Path; import subprocess; p=subprocess.run(["git","ls-files","--cached","--others","--exclude-standard","-z"],capture_output=True,check=True).stdout; bad=[]
for name in p.decode().split("\0"):
 if not name: continue
 q=Path(name)
 if q.suffix not in {".rs",".md",".py",".toml",".yml",".json",".sh",".luau"}: continue
 for i,line in enumerate(q.read_text().splitlines(),1):
  if line != line.rstrip(): bad.append(f"{name}:{i}: trailing whitespace")
print("\n".join(bad) if bad else "Source whitespace: clean")
raise SystemExit(bool(bad))'
git diff --check
printf '\nWorkspace verification passed. Live-agent and native-window gates are separate.\n'
