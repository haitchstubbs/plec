import { spawnSync } from "node:child_process";

const command =
  "yarn vitest run --config vitest.config.ts tests/benchmarks/issue53/report-capture.test.ts";
const result =
  process.platform === "win32"
    ? spawnSync("cmd.exe", ["/d", "/s", "/c", command], {
        cwd: process.cwd(),
        stdio: "inherit",
        env: {
          ...process.env,
          BENCH_QUERY_PARITY_REPORT: "1",
        },
      })
    : spawnSync("sh", ["-lc", command], {
        cwd: process.cwd(),
        stdio: "inherit",
        env: {
          ...process.env,
          BENCH_QUERY_PARITY_REPORT: "1",
        },
      });

if (result.error) {
  console.error(result.error);
  process.exit(1);
}
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}
