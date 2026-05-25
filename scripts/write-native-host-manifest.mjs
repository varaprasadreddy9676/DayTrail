import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const [browser, appPath, extensionId, outFile] = process.argv.slice(2);

if (!browser || !appPath || !extensionId || !outFile) {
  console.error("usage: write-native-host-manifest.mjs <chrome|brave|edge|firefox> <app-path> <extension-id> <out-file>");
  process.exit(1);
}

if (extensionId === "__EXTENSION_ID__" || extensionId.trim() === "") {
  console.error("extension id must be a real browser extension id, not a placeholder");
  process.exit(1);
}

const manifest = {
  name: "ai.daytrail.desktop",
  description: "DayTrail local browser bridge",
  path: appPath,
  type: "stdio",
};

if (browser === "chrome" || browser === "brave" || browser === "edge") {
  manifest.allowed_origins = [`chrome-extension://${extensionId}/`];
} else if (browser === "firefox") {
  manifest.allowed_extensions = ["daytrail-browser-bridge@example.com"];
} else {
  console.error(`unsupported browser: ${browser}`);
  process.exit(1);
}

mkdirSync(dirname(outFile), { recursive: true });
writeFileSync(outFile, `${JSON.stringify(manifest, null, 2)}\n`);
