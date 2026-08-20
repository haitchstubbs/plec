import { describe, it, expect } from 'vitest';
import { compile } from './index.js';

describe('Icon factory resolution', () => {
  it('should compile createIcon component to SVG island placeholder', () => {
    const source = `
      const createIcon = (definition) => (props) => ({
        type: 'svg',
        props: { viewBox: '0 0 24 24', ...props, children: definition }
      });

      const CircleHelpNode = [["circle",{"cx":"12","cy":"12","r":"10"}]];
      const CircleHelp = createIcon(CircleHelpNode);

      export function TestPage() {
        return <div><CircleHelp className="size-4" /></div>;
      }
    `;

    const result = compile(source, {
      moduleId: 'test-module',
      rootComponent: 'TestPage',
      mode: 'lenient',
    });

    // Verify compilation succeeded
    expect(result.diagnostics.length).toBe(0);

    // The icon should be compiled to a placeholder element in the executable format
    // (SVG islands use span placeholders with data-runtime-island attribute)
    const nodes = result.ir.nodes ?? [];
    const placeholderNode = nodes.find((node: any) => node.op === 'element' && result.ir.strings[node.tag] === 'span');
    expect(placeholderNode).toBeTruthy();
  });
});
