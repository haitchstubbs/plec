import { once } from "node:events";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createAppServer } from "./server";

const servers: Array<ReturnType<typeof createAppServer>> = [];
afterEach(async () => { await Promise.all(servers.splice(0).map((server) => new Promise<void>((resolve) => server.close(() => resolve())))); });

async function testServer() {
  const publicDir = await mkdtemp(path.join(tmpdir(), "o1-fullstack-"));
  await writeFile(path.join(publicDir, "index.html"), "<div id=app></div>");
  const server = createAppServer(publicDir);
  servers.push(server);
  server.listen(0);
  await once(server, "listening");
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("missing test port");
  return `http://127.0.0.1:${address.port}`;
}

describe("Node O1 adapter", () => {
  it("serves the SPA shell and CRUD Todo API", async () => {
    const origin = await testServer();
    expect(await (await fetch(`${origin}/about`)).text()).toContain("id=app");
    const initial = await (await fetch(`${origin}/api/todos`)).json();
    expect(initial).toHaveLength(1);
    const created = await (await fetch(`${origin}/api/todos`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ title: "Ship O1" }) })).json();
    expect(created.title).toBe("Ship O1");
    const updated = await (await fetch(`${origin}/api/todos/${created.id}`, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ completed: true }) })).json();
    expect(updated.completed).toBe(true);
    expect((await fetch(`${origin}/api/todos/${created.id}`, { method: "DELETE" })).status).toBe(204);
  });

  it("rejects invalid mutations and unknown API routes", async () => {
    const origin = await testServer();
    expect((await fetch(`${origin}/api/todos`, { method: "POST", headers: { "content-type": "application/json" }, body: "{}" })).status).toBe(400);
    expect((await fetch(`${origin}/api/todos/nope`, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ completed: "yes" }) })).status).toBe(404);
    expect((await fetch(`${origin}/api/nope`)).status).toBe(404);
  });
});
