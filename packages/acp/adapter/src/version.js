// Adapted for Zepi from @automatalabs/pi-acp 0.9.4 (Apache-2.0). See ../LICENSE.
import pkg from "../package.json" with { type: "json" };
export const PKG_VERSION = String(pkg.version);
