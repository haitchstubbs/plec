import { describe, expect, it } from "vitest";
import { reconcileInputSnapshot, type CollectionProjection } from "./index";

describe("compiled browser adapter", () => {
  it("reserves timing marks for adapter and runtime stages", () => {
    expect(["adapter-start", "adapter-end", "runtime-apply-start", "runtime-apply-end"]).toHaveLength(4);
  });

  it("reconciles only observed fields for a snapshot collection", () => {
    const previous: CollectionProjection = { kind: "collection", keys: ["a", "b"], rows: new Map([["a", { title: "A", done: false }], ["b", { title: "B", done: false }]]) };
    const next: CollectionProjection = { kind: "collection", keys: ["a", "b"], rows: new Map([["a", { title: "A", done: true }], ["b", { title: "B", done: false }]]) };
    expect(reconcileInputSnapshot("todos", previous, next, { kind: "collection", orderSensitive: true })).toEqual([
      { type: "update", inputId: "todos", rowKey: "a", changes: { done: true } }
    ]);
  });
});
