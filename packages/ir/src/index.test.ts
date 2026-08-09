import { describe, expect, it } from "vitest";
// Vitest loads source; package typecheck intentionally disallows this suffix.
// @ts-expect-error TypeScript Node resolution disallows .ts import suffixes.
import { renderStaticApplication, validateApplicationIr } from "./index.ts";

const base = {
  version: "0.4",
  rootElementId: "e1",
  elements: [
    { id: "e1", tag: "label", parentId: null, children: ["e2"] },
    { id: "e2", tag: "input", parentId: "e1", children: [] },
  ],
  texts: [],
  bindings: [],
  localStates: [{ id: "s1", name: "toggle-1", initialValue: "false", values: ["false", "true", "mixed"] }],
  events: [{ id: "ev1", type: "change", targetId: "e2", actionId: "a1" }],
};

describe("Toggle IR", () => {
  it("accepts a deterministic uncontrolled toggle", () => {
    expect(validateApplicationIr({ ...base, toggles: [{ id: "t1", rootElementId: "e1", inputElementId: "e2", stateSlotId: "s1", defaultChecked: { staticValue: "false" }, eventId: "ev1" }] }).toggles).toHaveLength(1);
  });

  it("rejects a toggle without a state source", () => {
    expect(() => validateApplicationIr({ ...base, toggles: [{ id: "t1", rootElementId: "e1", inputElementId: "e2", stateSlotId: "s1", eventId: "ev1" }] })).toThrow(/toggle requires checked or defaultChecked/);
  });
});

describe("static application renderer", () => {
  it("renders static nodes and host bindings while omitting query rows", () => {
    const ir = validateApplicationIr({
      version: "0.4", revision: "revision-1", rootElementId: "e1",
      elements: [
        { id: "e1", tag: "main", parentId: null, attributes: [], children: ["t1", "l1", "l2"] },
        { id: "e2", tag: "p", parentId: "e1", attributes: [], children: ["t2"] },
      ],
      texts: [{ id: "t1", parentId: "e1", staticValue: "Hello <world>" }, { id: "t2", parentId: "e2", staticValue: "static row" }],
      bindings: [], expressions: [], events: [],
      loops: [
        { id: "l1", parentId: "e1", source: "static", itemName: "item", rows: [{ id: "r1", rootElementId: "e2" }] },
        { id: "l2", parentId: "e1", source: "todos", itemName: "todo", queryId: "q1", rowTemplateRootElementId: "e2", rows: [] },
      ],
    });
    expect(renderStaticApplication(ir)).toBe('<main data-runtime-node="e1"><!--runtime-text:t1-->Hello &lt;world&gt;<p data-runtime-node="e2"><!--runtime-text:t2-->static row</p></main>');
  });

  it("evaluates pathname bindings deterministically", () => {
    const ir = validateApplicationIr({ version: "0.4", rootElementId: "e1", elements: [{ id: "e1", tag: "a", parentId: null, attributes: [{ name: "className", staticValue: "link" }], children: [] }], texts: [], bindings: [{ id: "b1", kind: "attribute", targetId: "e1", attributeName: "className", expressionId: "x1" }], expressions: [{ id: "x1", expression: { kind: "conditional", test: { kind: "binary", op: "===", left: { kind: "member", object: { kind: "member", object: { kind: "identifier", name: "host" }, property: "location" }, property: "pathname" }, right: { kind: "literal", value: "/active" } }, consequent: { kind: "literal", value: "active" }, alternate: { kind: "literal", value: "link" } } }], events: [] });
    expect(renderStaticApplication(ir, { location: { pathname: "/active" } })).toContain('class="active"');
  });
});
