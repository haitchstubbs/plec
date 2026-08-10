import { cp, mkdir, rm, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { build } from "esbuild";
import { compileSourceEntry } from "@wasm-runtime/compiler/node-entry";
import { validateApplicationIr } from "@wasm-runtime/ir";

const appDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoDir = path.resolve(appDir, "..", "..");
const require = createRequire(import.meta.url);
const distDir = path.join(appDir, "dist");
const publicDir = path.join(distDir, "public");
const o1Dir = path.join(publicDir, "o1");

const pages = [
  ["home", "src/routes/home.tsx", "HomePage"],
  ["about", "src/routes/about.tsx", "AboutPage"],
  ["not-found", "src/routes/not-found.tsx", "NotFoundPage"],
];
const layout = ["layout", "src/components/fullstack-layout.tsx", "FullstackLayout"];
let assetRevision = "development";

await rm(distDir, { recursive: true, force: true });
await mkdir(path.join(publicDir, "assets"), { recursive: true });
await mkdir(o1Dir, { recursive: true });

for (const [name, relativeSource, component] of [layout, ...pages]) {
  const compiled = await compileSourceEntry(path.join(appDir, relativeSource), { rootDir: appDir, repoRootDir: repoDir, mode: "strict", rootComponent: component });
  const result = compiled.result;
  if (name === "layout") assetRevision = compiled.revision;
  const errors = result.diagnostics.filter((diagnostic) => diagnostic.severity === "error");
  if (errors.length) throw new Error(`${relativeSource} failed O1 compilation:\n${errors.map((item) => `${item.code}: ${item.message}`).join("\n")}`);
  validateApplicationIr(result.ir);
  await writeFile(path.join(o1Dir, `${name}.ir.json`), `${JSON.stringify(result.ir, null, 2)}\n`);
}

await build({ entryPoints: [path.join(appDir, "src/client.ts")], bundle: true, format: "esm", platform: "browser", target: "es2022", outfile: path.join(publicDir, "assets/client.js") });
await build({ entryPoints: [path.join(appDir, "src/server.ts")], bundle: true, format: "esm", platform: "node", target: "node20", outfile: path.join(distDir, "server.mjs"), packages: "external" });

const tailwindCli = path.join(path.dirname(require.resolve("@tailwindcss/cli/package.json")), "dist", "index.mjs");
execFileSync(process.execPath, [tailwindCli, "-i", "src/styles.css", "-o", "dist/public/assets/styles.css"], { cwd: appDir, stdio: "inherit" });
await cp(path.join(repoDir, "packages/runtime/crates/runtime/dist/runtime"), path.join(o1Dir, "runtime"), { recursive: true });
await writeFile(path.join(publicDir, "index.html"), `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>O1 fullstack playground</title><link rel="stylesheet" href="/assets/styles.css?v=${assetRevision}"></head><body><div id="app" aria-live="polite"></div><script type="module" src="/assets/client.js?v=${assetRevision}"></script></body></html>\n`);
