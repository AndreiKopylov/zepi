"""Price a Zed jump per overlay file: zepi delta vs 1.22, upstream churn 1.22 to target.
Usage: python scripts/zed-port-matrix.py <pristine-old-dir> <pristine-new-dir>
Extract both from .local/downloads tarballs first, e.g.:
  tar -xzf .local/downloads/zed-76659a55.tar.gz -C <old-parent>
  tar -xzf .local/downloads/zed-trial-v1.26.2.tar.gz -C <new-parent>
Categories: PORT (zepi delta and upstream churn both nonzero), KEEP (upstream unchanged),
NEW (zepi-only file), DROP (overlay identical to old upstream), GONE (removed upstream).
Port cost heuristic: small zepi delta on big churn = reapply few lines onto new file (cheap);
big delta on big churn = semantic report (expensive)."""
import difflib
import sys
from collections import Counter
from pathlib import Path

NL = chr(10)
CR = chr(13)
overlay = Path(__file__).resolve().parent.parent / 'native/overlay'
p_old = Path(sys.argv[1])
p_new = Path(sys.argv[2])


def lines(path):
    return path.read_text(encoding='utf-8', errors='replace').replace(CR, '').splitlines()


def diffcount(a, b):
    count = 0
    for line in difflib.unified_diff(a, b, lineterm=''):
        if line.startswith('+') and not line.startswith('+++'):
            count += 1
        elif line.startswith('-') and not line.startswith('---'):
            count += 1
    return count


rows = []
for path in sorted(overlay.rglob('*')):
    if not path.is_file():
        continue
    rel = path.relative_to(overlay).as_posix()
    if not rel.startswith('crates/'):
        continue
    a, b = p_old / rel, p_new / rel
    if not a.is_file():
        rows.append(('NEW', rel, diffcount([], lines(path)), 0))
    elif not b.is_file():
        rows.append(('GONE', rel, diffcount(lines(a), lines(path)), 0))
    else:
        delta = diffcount(lines(a), lines(path))
        churn = diffcount(lines(a), lines(b))
        if delta == 0:
            rows.append(('DROP', rel, 0, churn))
        elif churn == 0:
            rows.append(('KEEP', rel, delta, 0))
        else:
            rows.append(('PORT', rel, delta, churn))

order = {'PORT': 0, 'GONE': 1, 'NEW': 2, 'KEEP': 3, 'DROP': 4}
rows.sort(key=lambda r: (order[r[0]], -r[3]))
print(Counter(r[0] for r in rows))
for kind, rel, delta, churn in rows:
    if kind in ('PORT', 'GONE'):
        print(f'{kind} {rel} zepi_delta={delta} upstream_churn={churn}')
print('--- NEW (zepi-only):')
for kind, rel, delta, churn in rows:
    if kind == 'NEW':
        print(f'NEW {rel} lines={delta}')
print('--- DROP candidates:', sum(1 for r in rows if r[0] == 'DROP'))
print('--- KEEP as-is:', sum(1 for r in rows if r[0] == 'KEEP'))
