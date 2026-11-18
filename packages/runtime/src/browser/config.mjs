import { readFile } from "node:fs/promises";
import { join } from "node:path";

const settings = async directory => (JSON.parse(await readFile(join(directory, "settings.json"), "utf8").catch(error => {
  if (error.code === "ENOENT") return "{}"; throw error;
}))).zepi ?? {};

// Browser backend selection; browse (real browser + extension) is the only engine.
export async function browserEnvironment(directory) {
  const config = (await settings(directory)).browser ?? {};
  return {
    backend: "browse",
    browseDir: config.browseDir ?? join(directory, "browse"),
    browseTools: config.browseTools === "plain" ? "plain" : "execute",
    browseCli: config.browseCli ?? "",
  };
}