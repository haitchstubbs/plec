import path from "node:path";
import { describe, expect, it } from "vitest";
import { compileSourceEntry } from "@wasm-runtime/compiler/node-entry";
import { validateApplicationIr } from "@wasm-runtime/ir";

const pages = [
  ["home", "HomePage"],
  ["about", "AboutPage"],
  ["not-found", "NotFoundPage"],
] as const;

describe("O1 page compilation", () => {
  for (const [name, component] of pages) {
    it(`compiles ${name} deterministically without diagnostics`, async () => {
      const options = { rootDir: path.resolve("."), repoRootDir: path.resolve("..", ".."), mode: "strict" as const, rootComponent: component };
      const first = await compileSourceEntry(path.resolve("src/routes", `${name}.tsx`), options);
      const second = await compileSourceEntry(path.resolve("src/routes", `${name}.tsx`), options);
      expect(first.result.diagnostics).toEqual([]);
      expect(validateApplicationIr(first.result.ir).rootElementId).toBe("e1");
      expect(first.result.ir).toEqual(second.result.ir);
    });
  }

  it("compiles the persistent layout with explicit sidebar and outlet metadata", async () => {
    const compiled = await compileSourceEntry(path.resolve("src/components/fullstack-layout.tsx"), { rootDir: path.resolve("."), repoRootDir: path.resolve("..", ".."), mode: "strict", rootComponent: "FullstackLayout" });
    expect(compiled.result.diagnostics).toEqual([]);
    expect(compiled.result.ir.layout).toMatchObject({ routeOutlets: [{ id: "main" }], sidebar: { persistenceKey: "sidebar_state", linkElementIds: expect.arrayContaining([expect.any(String)]) } });
  });
});
