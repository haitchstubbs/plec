import { describe, expect, it, vi } from "vitest";
import { markO1Timing, reconcileInputSnapshot, type CollectionProjection } from "./index";

describe("compiled browser adapter", () => {
  it("emits named O1 mount boundaries through User Timing", () => {
    const mark = vi.fn();
    vi.stubGlobal("performance", { mark });
    markO1Timing("o1:artifact-fetch-start");
    markO1Timing("o1:mount-end");
    markO1Timing("o1:mount-error");
    expect(mark.mock.calls).toEqual([["o1:artifact-fetch-start"], ["o1:mount-end"], ["o1:mount-error"]]);
    vi.unstubAllGlobals();
  });

  it("reconciles only observed fields for a snapshot collection", () => {
    const previous: CollectionProjection = { kind: "collection", keys: ["a", "b"], rows: new Map([["a", { title: "A", done: false }], ["b", { title: "B", done: false }]]) };
    const next: CollectionProjection = { kind: "collection", keys: ["a", "b"], rows: new Map([["a", { title: "A", done: true }], ["b", { title: "B", done: false }]]) };
    expect(reconcileInputSnapshot("todos", previous, next, { kind: "collection", orderSensitive: true })).toEqual([
      { type: "update", inputId: "todos", rowKey: "a", changes: { done: true } }
    ]);
  });
});
