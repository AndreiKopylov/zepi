// Each command has its own process group. IPC disconnect kills that group even
// if the native terminal force-kills the preview supervisor without a signal handler.
import {spawn, spawnSync} from "node:child_process";

function killGroup() {
  if (process.platform === "win32") {
    // No process groups on Windows: kill our own tree so the supervised command dies with us.
    // shortcut: taskkill /T misses breakaway descendants; upgrade to Job Objects if orphans appear.
    try { spawnSync("taskkill", ["/T", "/F", "/PID", String(process.pid)], { stdio: "ignore", windowsHide: true }); }
    catch { /* best effort */ }
    process.exit(1);
  }
  try { process.kill(-process.pid, "SIGKILL"); }
  catch { process.exit(1); }
}
process.on("disconnect", killGroup);
process.on("SIGTERM", killGroup);
process.on("SIGINT", killGroup);
process.on("SIGHUP", killGroup);
process.once("message", spec => {
  let finished = false;
  function finish(result) {
    if (finished) return;
    finished = true;
    if (process.connected) process.send(result, killGroup);
    else killGroup();
  }
  const child = spawn(spec.command, spec.args, {stdio: ["ignore", "inherit", "inherit"]});
  child.once("error", error => finish({exitCode: null, error: error.message}));
  child.once("exit", (exitCode, signal) => finish({exitCode, ...(signal ? {signal} : {})}));
});
