#!/usr/bin/env bash
# Launcher: invokes win-build.cmd outside MSYS so INCLUDE/LIB are not mangled by path conversion.
d="$(cd "$(dirname "$0")" && pwd)"
cmd //c "$(cygpath -w "$d/win-build.cmd")" > "$d/.local/build2.log" 2>&1
echo "exit=$?"
