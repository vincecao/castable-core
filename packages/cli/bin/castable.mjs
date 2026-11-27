#!/usr/bin/env node
// Runs the TypeScript entry through tsx, so there is no build step.
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const tsx = require.resolve("tsx/cli");
const entry = fileURLToPath(new URL("../src/cli.tsx", import.meta.url));
const { status } = spawnSync(process.execPath, [tsx, entry, ...process.argv.slice(2)], { stdio: "inherit" });
process.exit(status ?? 1);
