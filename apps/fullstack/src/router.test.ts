import { describe, expect, it } from "vitest";
import { artifactForPath, createO1Router } from "./router";

describe("O1 router", () => {
  it("maps core router matches to compiled artifacts", () => {
    const router = createO1Router();
    expect(artifactForPath(router, "/")).toBe("home");
    expect(artifactForPath(router, "/about")).toBe("about");
    expect(artifactForPath(router, "/missing")).toBe("not-found");
  });
});
