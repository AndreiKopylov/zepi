import { AsyncLocalStorage } from "node:async_hooks";
import { readFile as readFileFromDisk, writeFile as writeFileFromDisk, stat } from "node:fs/promises";
import { readableFile, writableFile } from "./workspace-path.ts";
import { methods, type AgentContext } from "@agentclientprotocol/sdk";
import {
  createReadToolDefinition,
  createEditToolDefinition,
  createWriteToolDefinition,
  defineTool,
  detectSupportedImageMimeTypeFromFile,
} from "@earendil-works/pi-coding-agent";

export function editorTools(cwd: string, sessionId: string, client: Pick<AgentContext, "request">,
  outputImage: (path:string)=>{data:string;mimeType:string}|undefined = ()=>undefined) {
  const readPaths = new Set<string>();
  const signals = new AsyncLocalStorage<AbortSignal | undefined>();
  const checkPath = (path: string) => writableFile(cwd, path);
  const imagePaths = new Set<string>();
  const detectImageMimeType = async (path: string) => {
    const output = outputImage(path); if (output) return output.mimeType;
    // Reads may target skills and commands outside the project.
    // Missing files cannot be images: let the read/write itself report ENOENT.
    const canonical = await readableFile(cwd, path).then(file => file.canonical).catch((error: NodeJS.ErrnoException) => {
      if (error.code === 'ENOENT') return null;
      throw error;
    });
    if (!canonical) return null;
    // The public pi detector only sniffs the header. Read actual image contents
    // through the native snapshot endpoint, with its exclusions and size bound.
    const mime = await detectSupportedImageMimeTypeFromFile(canonical).catch((error: NodeJS.ErrnoException) => {
      if (error.code === 'ENOENT') return null; // new unsaved editor file
      throw error;
    });
    if (mime) imagePaths.add(canonical); else imagePaths.delete(canonical);
    return mime;
  };
  // Missing files may live only in native buffers (created but unsaved): resolve the
  // workspace boundary tolerantly, then read natively. Disk reads stay exact.
  const locate = async (path: string) => {
    try { return await readableFile(cwd, path); }
    catch (error) {
      if ((error as Error).message.startsWith('Private workspace storage')) throw error;
      return writableFile(cwd, path);
    }
  };
  const readFile = async (path: string) => {
    const output = outputImage(path); if (output) return Buffer.from(output.data, 'base64');
    const file = await locate(path);
    // Skills and commands live outside the project as plain files: no buffer, no snapshot.
    if (!file.insideWorkspace) { readPaths.add(file.canonical); return readFileFromDisk(file.canonical); }
    const canonical = file.canonical;
    if (imagePaths.has(canonical)) {
      const response = await client.request<{content:string}>('_zepi/fs/snapshot', {sessionId, path:canonical}, {cancellationSignal:signals.getStore()});
      const content = Buffer.from(response.content, 'base64');
      if (content.toString('base64') !== response.content) throw new Error('Invalid native image snapshot.');
      return content;
    }
    const response = await client.request(methods.client.fs.readTextFile, { sessionId, path: canonical }, { cancellationSignal: signals.getStore() });
    readPaths.add(canonical);
    return Buffer.from(response.content, "utf8");
  };
  const writeFile = async (path: string, content: string) => {
    const { canonical, insideWorkspace } = await checkPath(path);
    const exists = await stat(canonical).then(() => true, (error: NodeJS.ErrnoException) => {
      if (error.code !== "ENOENT") throw error;
      return false;
    });
    if (exists && !readPaths.has(canonical)) throw new Error("Read the file in this session before replacing its contents.");
    if (!insideWorkspace) {
      await writeFileFromDisk(canonical, content, "utf8");
      readPaths.add(canonical);
      return;
    }
    await client.request(methods.client.fs.writeTextFile, { sessionId, path: canonical, content,
      ...(!exists ? {_meta: {"zepi.dev/createFile": true}} : {}),
    }, { cancellationSignal: signals.getStore() });
    readPaths.add(canonical);
  };
  const access = async (path: string) => {
    if (outputImage(path)) return;
    try { await readableFile(cwd, path); }
    catch (error) {
      // Writes may leave the workspace; private storage stays restricted.
      if ((error as Error).message.startsWith('Private workspace storage')) throw error;
      await writableFile(cwd, path);
    }
  };
  return [
    defineTool(createReadToolDefinition(cwd, { operations: { readFile, access, detectImageMimeType } })),
    defineTool(createEditToolDefinition(cwd, { operations: { readFile, writeFile, access } })),
    defineTool(createWriteToolDefinition(cwd, { operations: { writeFile, mkdir: async () => {} } })),
  ].map(tool => ({ ...tool, execute: (...args: Parameters<typeof tool.execute>) => signals.run(args[2], () => tool.execute(...args)) }));
}
