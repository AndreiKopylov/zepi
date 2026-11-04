"""Port the zepi overlay onto a new Zed base by three-way merging each file.

ours = native/overlay/<rel>, base = pristine old Zed, theirs = pristine new Zed
(Cargo.toml/Cargo.lock excluded - they are merged by zed-update.py project rules).
Conflict markers stay in the overlay file for manual resolution; the report lists them.
Usage: python scripts/zed-port-merge.py <pristine-old-dir> <pristine-new-dir> [--dry-run] [--file <rel>]
"""
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OVERLAY = ROOT / 'native' / 'overlay'
SKIP = {'Cargo.toml', 'Cargo.lock'}


def merge(ours, base, theirs, scratch):
    """git merge-file on LF copies; returns (merged text, conflicts) or None when binary."""
    try:
        text = ours.read_text(encoding='utf-8').replace('\r\n', '\n')
    except UnicodeDecodeError:
        return None
    scratch.write_text(text, encoding='utf-8', newline='\n')
    result = subprocess.run(['git', 'merge-file', '-p', '--diff3', '-L', 'ours', '-L', 'base', '-L', 'theirs',
                             str(scratch), str(base), str(theirs)], capture_output=True)
    merged = result.stdout.decode('utf-8', 'replace')
    return merged, merged.count('<<<<<<< ours')


def main():
    argv = sys.argv[1:]
    single = argv[argv.index('--file') + 1] if '--file' in argv else None
    if single:
        argv = argv[:argv.index('--file')] + argv[argv.index('--file') + 2:]
    args = [a for a in argv if not a.startswith('--')]
    if len(args) != 2:
        sys.exit(__doc__)
    apply_changes = '--dry-run' not in argv
    old, new = Path(args[0]), Path(args[1])
    scratch = Path(tempfile.mkdtemp()) / 'ours.txt'
    clean, conflicts, added, gone, binary = [], [], [], [], []
    for path in sorted(OVERLAY.rglob('*')):
        if not path.is_file():
            continue
        relative = path.relative_to(OVERLAY).as_posix()
        if not relative.startswith('crates/') or relative in SKIP:
            continue
        if single and relative != single:
            continue
        base, theirs = old / relative, new / relative
        if not base.is_file():
            added.append(relative)
            continue
        if not theirs.is_file():
            gone.append(relative)
            continue
        result = merge(path, base, theirs, scratch)
        if result is None:
            binary.append(relative)
            continue
        merged, count = result
        if count:
            conflicts.append((relative, count))
        else:
            clean.append(relative)
        if apply_changes:
            path.write_text(merged, encoding='utf-8', newline='\n')
    print('clean', len(clean), 'conflicts', len(conflicts), 'new', len(added),
          'gone', len(gone), 'binary', len(binary), flush=True)
    print('conflict hunks:', sum(count for _, count in conflicts), flush=True)
    for relative, count in sorted(conflicts, key=lambda item: -item[1]):
        print(f'{count:4d}  {relative}', flush=True)
    for relative in gone:
        print('GONE upstream, kept as-is:', relative, flush=True)


if __name__ == '__main__':
    main()
