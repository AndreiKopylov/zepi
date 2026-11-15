// Single place the bundled pi is imported. Literal specifiers let bun embed pi into
// agent.exe at compile time; under node/tsx the same paths resolve on disk.
export {
  DefaultPackageManager, SettingsManager, parseFrontmatter,
  ModelRuntime, readStoredCredential,
} from "../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/index.js";
export { FileSettingsStorage } from "../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/core/settings-manager.js";
export { AuthStorage, ReadOnlyAuthStorage } from "../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js";
export { ModelConfig } from "../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/core/model-config.js";
export { getSupportedThinkingLevels } from "../../../acp/node_modules/@earendil-works/pi-ai/dist/index.js";
export { takeOverStdout, restoreStdout, writeRawStdout, flushRawStdout } from "../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/core/output-guard.js";
export { KeybindingsManager } from "../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/core/keybindings.js";
export { default as piManifest } from "../../../acp/node_modules/@earendil-works/pi-coding-agent/package.json" with { type: "json" };