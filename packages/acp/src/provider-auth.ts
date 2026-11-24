import { join } from "node:path";
import { createInterface, type Interface } from "node:readline";
import { ModelRuntime, SettingsManager, flushRawStdout, takeOverStdout, writeRawStdout } from "../../runtime/src/pi/embed.mjs";
import { piDirectory } from "../../runtime/src/paths.mjs";
import type { AuthInteraction } from "@earendil-works/pi-ai";
import { initializePiNetwork } from "./pi-network.ts";

export function providerAuthInteraction(input: Pick<Interface, "on">, send: (event: object) => void, signal: AbortSignal): AuthInteraction {
  let sequence = 0;
  let respond: ((id: unknown, value: unknown) => void) | undefined;
  input.on("line", line => {
    if (!respond || line.length > 64_000) return;
    let reply;
    try { reply = JSON.parse(line); } catch { return; }
    if (reply && typeof reply === "object") respond(reply.id, reply.value);
  });
  return {
    signal,
    async prompt({signal: promptSignal, ...prompt}) {
      const stop = promptSignal ? AbortSignal.any([signal, promptSignal]) : signal;
      stop.throwIfAborted();
      if (respond) throw new Error("Another sign-in prompt is active.");
      const id = ++sequence;
      return new Promise<string>((resolve, reject) => {
        const clear = () => {
          respond = undefined;
          stop.removeEventListener("abort", cancel);
          send({clearPrompt: id});
        };
        const cancel = () => { clear(); reject(stop.reason); };
        stop.addEventListener("abort", cancel, {once: true});
        respond = (received, value) => {
          if (received !== id || typeof value !== "string" || value.length > 8192) return;
          if (prompt.type === "select" && !prompt.options.some(option => option.id === value)) return;
          if (prompt.type !== "text" && !value.trim()) return;
          clear();
          resolve(value);
        };
        send({prompt: {...prompt, id}});
      });
    },
    notify(event) {
      if (signal.aborted) return;
      if (event.type === "auth_url") send({url: event.url});
      if (event.type === "device_code") send({url: event.verificationUri, code: event.userCode,
        autoOpen: event.openBrowserOnConfirm !== false, copyCode: event.copyCodeOnConfirm !== false});
    },
  };
}

export async function runProviderSignIn(id: string | undefined) {
  takeOverStdout();
  const send = (event: object) => writeRawStdout(JSON.stringify(event) + "\n");
  const stop = new AbortController();
  const input = createInterface({input: process.stdin, crlfDelay: Infinity});
  // The settings window owns stdin and closing it cancels the flow.
  let emergency: ReturnType<typeof setTimeout> | undefined;
  const cancel = () => {
    if (stop.signal.aborted) return;
    stop.abort();
    emergency = setTimeout(() => process.exit(1), 5_000);
  };
  input.once("close", cancel);
  process.once("SIGTERM", cancel);
  const deadline = setTimeout(cancel, 5 * 60_000);
  try {
    const directory = piDirectory();
    initializePiNetwork(directory);
    const settings = SettingsManager.create(directory, directory, {projectTrusted: false});
    const runtime = await ModelRuntime.create({authPath: join(directory, "auth.json"),
      modelsPath: join(directory, "models.json"), refreshOnCreate: false, allowModelNetwork: false});
    if (!id || runtime.getError() || !runtime.getProvider(id)?.auth.oauth?.login) {
      send({ok: false, error: "Account sign-in is unavailable for this provider."});
      return;
    }
    await runtime.login(id, "oauth", providerAuthInteraction(input, send, stop.signal),
      {getDeviceId: () => settings.getOrCreateDeviceId()});
    await settings.flush();
    send({ok: true});
  } catch {
    send({ok: false, error: stop.signal.aborted ? "Sign-in cancelled." : "Provider sign-in failed. Check the provider settings and try again."});
  } finally {
    clearTimeout(deadline);
    clearTimeout(emergency);
    input.removeListener("close", cancel);
    input.close();
    process.stdin.pause();
    process.removeListener("SIGTERM", cancel);
    await flushRawStdout();
  }
}
