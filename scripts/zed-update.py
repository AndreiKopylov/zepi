"""Price a Zed version jump: scratch tree + our overlay + cargo check, error report per crate and per our file."""
import hashlib, importlib.util, json, os, re, shutil, subprocess, sys
from pathlib import Path
NL = chr(10)

ROOT = Path(__file__).resolve().parent.parent
LOCAL = ROOT / '.local'
WORK = Path(os.environ.get('ZEPI_WORK_ROOT', str(ROOT.parent)))
SCRATCH_ROOT = WORK / 'zed-trial'
TARGET = WORK / 'zed-trial-target'
OUR_FILES = [
    'crates/acp_thread/src/zepi_file_review.rs',
    'crates/agent_servers/src/custom.rs',
    'crates/agent_ui/src/agent_panel.rs',
    'crates/agent_ui/src/conversation_view/thread_view.rs',
    'crates/agent_ui/src/zepi_access.rs',
    'crates/zepi_ui/src/zepi_ui.rs',
    'crates/extensions_ui/src/pi_extensions.rs',
    'crates/settings_ui/src/pi_settings.rs',
    'crates/workspace/src/dock.rs',
    'crates/workspace/src/workspace.rs',
    'crates/zed/src/zepi_runtime.rs',
]
MSVC = 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC/14.44.35207'
SDK = 'C:/Program Files (x86)/Windows Kits/10'
SDKV = '10.0.26100.0'


def digest(path):
    hasher = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(block)
    return hasher.hexdigest()


def load_native():
    spec = importlib.util.spec_from_file_location('zepi_native', ROOT / 'scripts' / 'native.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def lock_text_version(text, crate):
    match = re.search(r'name = "' + crate + r'"\s+version = "([^"]+)"', text)
    return match.group(1) if match else None


def main():
    if len(sys.argv) != 2:
        sys.exit('Usage: python scripts/zed-update.py <tag-or-sha>')
    ref = sys.argv[1]
    native = load_native()
    downloads = LOCAL / 'downloads'
    downloads.mkdir(parents=True, exist_ok=True)
    archive = downloads / ('zed-trial-' + ref + '.tar.gz')
    if not archive.exists():
        url = 'https://codeload.github.com/zed-industries/zed/tar.gz/' + ref
        print('Downloading', url, flush=True)
        subprocess.run(['curl', '-fL', '--retry', '3', '--connect-timeout', '20', '-o', str(archive) + '.partial', url], check=True)
        (archive.parent / (archive.name + '.partial')).rename(archive)
    sha = digest(archive)
    (LOCAL / ('zed-trial-' + ref + '.sha256')).write_text(sha + '\n')
    print('Archive sha256:', sha, flush=True)

    scratch = SCRATCH_ROOT / ref
    if scratch.exists():
        shutil.rmtree(scratch)
    scratch.mkdir(parents=True)
    subprocess.run(['tar', '-xzf', str(archive), '-C', str(scratch)], check=True)
    tree = next(scratch.iterdir())
    print('Extracted', tree.name, flush=True)

    overlay = ROOT / 'native/overlay'
    # The overlay owns the 1.22-era root Cargo.toml/Cargo.lock; on a new Zed they must be
    # merged, not copied: new workspace definition plus zepi crates and dependencies.
    new_root_toml = (tree / 'Cargo.toml').read_text(encoding='utf-8')
    copied = 0
    for relative in native.source_files():
        if relative in ('Cargo.toml', 'Cargo.lock'):
            continue
        source = overlay / relative
        destination = tree / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        copied += 1

    removed_now = set(native.removed_paths())
    previous_state = LOCAL / 'native-applied.json'
    if previous_state.exists():
        import json as _json
        removed_now |= {key for key, value in _json.loads(previous_state.read_text()).get('files', {}).items() if value is None}
    for relative in sorted(removed_now):
        target = tree / relative
        if target.is_file():
            target.unlink()
    fixes = WORK / 'zed-trial-fixes' / ref
    applied_fixes = 0
    if fixes.is_dir():
        for path in fixes.rglob('*'):
            if not path.is_file():
                continue
            relative = path.relative_to(fixes).as_posix()
            destination = tree / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
            applied_fixes += 1
    print('Trial fixes applied:', applied_fixes, flush=True)
    overlay_root = (overlay / 'Cargo.toml').read_text(encoding='utf-8')

    def members(text):
        block = re.search(r'members = \[(.*?)\]', text, re.S)
        return re.findall(r'"([^"]+)"', block.group(1)) if block else []

    def workspace_deps(text):
        result = {}
        inside = False
        for line in text.splitlines():
            stripped = line.strip()
            if stripped == '[workspace.dependencies]':
                inside = True
                continue
            if inside and stripped.startswith('['):
                break
            if inside:
                match = re.match(r'([A-Za-z0-9_-]+)\s*=', stripped)
                if match:
                    result[match.group(1)] = stripped
        return result

    new_members, overlay_members = members(new_root_toml), members(overlay_root)
    extra_members = [m for m in overlay_members if m not in new_members and (tree / m / 'Cargo.toml').is_file()]
    new_deps, overlay_deps = workspace_deps(new_root_toml), workspace_deps(overlay_root)
    missing_deps = [overlay_deps[key] for key in overlay_deps if key not in new_deps]
    merged = new_root_toml
    exclude_match = re.search(r'^exclude = \[.*?\]', overlay_root, re.M)
    if exclude_match and 'exclude =' not in merged:
        merged = merged.replace('resolver = "2"', 'resolver = "2"' + NL + exclude_match.group(0), 1)
    if extra_members:
        merged = re.sub(r'members = \[', 'members = [' + NL + '    ' + (',' + NL + '    ').join('"' + m + '"' for m in extra_members) + ',', merged, count=1)
    if missing_deps:
        merged = merged.replace('[workspace.dependencies]', '[workspace.dependencies]' + NL + NL.join(missing_deps), 1)
    (tree / 'Cargo.toml').write_text(merged, encoding='utf-8')
    # Keep the new Zed lock: incremental resolution preserves its pins (merman and friends)
    # and only adds what the zepi crates need.
    trial_lock = (tree / 'Cargo.lock').read_text(encoding='utf-8', errors='replace') if (tree / 'Cargo.lock').is_file() else ''
    print('Overlay applied:', copied, 'files,', len(removed_now), 'removals,',
          len(extra_members), 'extra members,', len(missing_deps), 'extra workspace deps', flush=True)

    env = dict(os.environ)
    env['RUSTUP_TOOLCHAIN'] = '1.98.1-x86_64-pc-windows-msvc'
    env['RUSTUP_AUTO_INSTALL'] = '0'
    env['CARGO_TARGET_DIR'] = str(TARGET)
    env['CARGO_NET_OFFLINE'] = 'false'
    env['CARGO_NET_GIT_FETCH_WITH_CLI'] = 'true'
    env['CARGO_BUILD_JOBS'] = '4'
    env['CRAFT_I18N_DIR'] = str(ROOT / 'native' / 'overlay' / 'vendor' / 'craft-i18n')
    env['ZED_COMMIT_SHA'] = ref + '-zepi-trial'
    env['LK_DEBUG_WEBRTC'] = 'false'
    env['INCLUDE'] = os.pathsep.join([MSVC + '/include', SDK + '/Include/' + SDKV + '/ucrt', SDK + '/Include/' + SDKV + '/um', SDK + '/Include/' + SDKV + '/shared', SDK + '/Include/' + SDKV + '/winrt', SDK + '/Include/' + SDKV + '/cppwinrt', str(LOCAL / 'msvc-shim')])
    env['LIB'] = os.pathsep.join([MSVC + '/lib/x64', SDK + '/Lib/' + SDKV + '/ucrt/x64', SDK + '/Lib/' + SDKV + '/um/x64'])
    extra_default = str(WORK / 'tools' / 'cmake-3.30.5-windows-x86_64' / 'bin') + os.pathsep + str(WORK / 'tools' / 'nasm-2.16.03')
    extra = [p for p in os.environ.get('ZEPI_EXTRA_PATH', extra_default).split(os.pathsep) if p]
    env['PATH'] = os.pathsep.join([MSVC + '/bin/Hostx64/x64'] + extra + [env['PATH']])
    webrtc = LOCAL / 'upstream' / 'zed' / '.webrtc-sys'
    current_version = lock_text_version(trial_lock, 'webrtc-sys')
    pinned_lock = (ROOT / 'native' / 'overlay' / 'Cargo.lock').read_text(encoding='utf-8', errors='replace')
    pinned_version = lock_text_version(pinned_lock, 'webrtc-sys')
    if current_version and current_version == pinned_version and webrtc.exists():
        candidate = next(webrtc.glob('webrtc-*/win-x64-release'), None)
        if candidate:
            env['LK_CUSTOM_WEBRTC'] = str(candidate)
            print('Reusing webrtc assets for webrtc-sys', current_version, flush=True)
    if 'LK_CUSTOM_WEBRTC' not in env:
        print('WARNING: webrtc-sys', current_version, '!= pinned', pinned_version, '- build script will try its own download', flush=True)

    log_path = LOCAL / ('zed-update-' + ref + '.log')
    print('Running cargo check -p zed (long). Log:', log_path, flush=True)
    with log_path.open('w', encoding='utf-8', errors='replace') as log:
        process = subprocess.run(['cargo', 'check', '--keep-going', '-p', 'zed', '--features', 'gpui_platform/runtime_shaders'],
                                 cwd=tree, env=env, stdout=log, stderr=subprocess.STDOUT)
    report = log_path.read_text(encoding='utf-8', errors='replace')
    errors = []
    lines = report.splitlines()
    for index, line in enumerate(lines):
        if line.startswith('error[') or line.startswith('error:'):
            location = ''
            for follow in lines[index + 1:index + 6]:
                match = re.search(r'--> (.+?)(?::\d+)?$', follow.strip())
                if match:
                    location = match.group(1)
                    break
            errors.append((line.strip()[:110], location))
    by_crate = {}
    ours = []
    for message, location in errors:
        match = re.search(r'crates[\\/]([^\\/]+)', location)
        crate = match.group(1) if match else '(other)'
        by_crate[crate] = by_crate.get(crate, 0) + 1
        if any(location.replace('\\', '/').endswith(one) for one in OUR_FILES):
            ours.append((location, message))
    print('cargo exit code:', process.returncode, flush=True)
    print('Total error blocks:', len(errors), flush=True)
    print('Errors per crate:', json.dumps(dict(sorted(by_crate.items(), key=lambda kv: -kv[1])), ensure_ascii=False), flush=True)
    print('Errors in our Windows-layer files:', len(ours), flush=True)
    for location, message in ours[:25]:
        print('  OURS', location, '|', message, flush=True)
    if process.returncode == 0:
        print('TRIAL OK: overlay compiles against', ref, flush=True)


if __name__ == '__main__':
    main()
