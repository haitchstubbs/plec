import { spawnSync } from "node:child_process";

const label = process.argv[2];

if (label !== "baseline" && label !== "post") {
  console.error(
    "Usage: node ./scripts/run-query-parity-benchmark.mjs <baseline|post>",
  );
  process.exit(1);
}

const command =
  "yarn vitest run --config vitest.config.ts tests/benchmarks/issue53/capture.test.ts";
const result =
  process.platform === "win32"
    ? spawnSync("cmd.exe", ["/d", "/s", "/c", command], {
        cwd: process.cwd(),
        stdio: "inherit",
        env: {
          ...process.env,
          BENCH_QUERY_PARITY_CAPTURE: "1",
          BENCH_QUERY_PARITY_LABEL: label,
        },
      })
    : spawnSync("sh", ["-lc", command], {
        cwd: process.cwd(),
        stdio: "inherit",
        env: {
          ...process.env,
          BENCH_QUERY_PARITY_CAPTURE: "1",
          BENCH_QUERY_PARITY_LABEL: label,
        },
      });

if (result.error) {
  console.error(result.error);
  process.exit(1);
}
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}
