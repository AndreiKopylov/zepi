// Reproduces the "Zepi does not reopen / leaks processes on close" regression:
// launch -> window appears -> graceful close -> process+children gone -> relaunch opens.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';

const exe = process.env.ZEPI_TEST_EXE || 'C:/Llm/zepi/zepi.exe';
const win = process.platform === 'win32';

function ps(cmd) {
  const r = spawnSync('powershell', ['-NoProfile', '-Command', cmd], { encoding: 'utf8' });
  return (r.stdout || '').trim();
}
const procCount = (pid) => Number(ps(`@(Get-Process -Id ${pid} -ErrorAction SilentlyContinue).Count`));
const windowTitle = (pid) => ps(`(Get-Process -Id ${pid} -ErrorAction SilentlyContinue).MainWindowTitle`);
const zepiCount = () => Number(ps(`@(Get-Process zepi -ErrorAction SilentlyContinue).Count`));
const killTree = (pid) => spawnSync('taskkill', ['/T', '/F', '/PID', String(pid)], { encoding: 'utf8' });

async function waitFor(fn, timeoutMs, step = 500) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    if (fn()) return true;
    await delay(step);
  }
  return false;
}

test('zepi opens, closes without leaking, and reopens', { timeout: 90_000, skip: !win || !existsSync(exe) }, async () => {
  assert.equal(zepiCount(), 0, 'a running Zepi holds the single-instance mutex; close it before the test');

  const first = spawn(exe, [], { detached: true, stdio: 'ignore' });
  first.unref();
  const opened = await waitFor(() => windowTitle(first.pid) !== '', 25_000);
  assert.ok(opened, 'first launch must create a window');

  // Close via the title-bar X (WM_SYSCOMMAND/SC_CLOSE), exactly as a user does.
  ps(`Add-Type -Name U -Namespace W -MemberDefinition '[DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);'; $h=(Get-Process -Id ${first.pid}).MainWindowHandle; [W.U]::SendMessage($h,0x0112,[IntPtr]0xF060,[IntPtr]0)`);
  const exited = await waitFor(() => procCount(first.pid) === 0, 15_000);
  assert.ok(exited, 'zepi must exit on close (no zombie holding the mutex)');
  const childrenGone = await waitFor(() => Number(ps(`@(Get-CimInstance Win32_Process -Filter "Name='agent.exe' AND ParentProcessId=${first.pid}").Count`)) === 0, 10_000);
  assert.ok(childrenGone, 'agent children must be reaped on close');

  const second = spawn(exe, [], { detached: true, stdio: 'ignore' });
  second.unref();
  const reopened = await waitFor(() => windowTitle(second.pid) !== '', 25_000);
  assert.ok(reopened, 'second launch must open (mutex released)');
  killTree(second.pid);
});