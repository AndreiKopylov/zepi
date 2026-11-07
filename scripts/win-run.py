#!/usr/bin/env python3
# Run zepi from the repo: configs live in .zed/ and .pi/agent at the root, the exe seeds them.
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUILD_DIR = Path(os.environ.get("ZEPI_BUILD_DIR", str(ROOT.parent / "zepi-build")))
exe = BUILD_DIR / "release-fast" / "zepi.exe"
if not exe.exists():
    exe = BUILD_DIR / "debug" / "zepi.exe"
env = dict(os.environ)
env["ZEPI_NATIVE_SOURCE_ROOT"] = str(ROOT / ".local" / "upstream" / "zed")
subprocess.run("npm run build:acp", cwd=ROOT, shell=True, check=True)
log = open(ROOT / ".local" / "run.log", "ab", buffering=0)
subprocess.Popen([str(exe)], env=env, cwd=str(ROOT / ".local" / "upstream" / "zed"), stdout=log, stderr=log)
print("launched", exe)
