import { afterEach, describe, expect, it, vi } from 'vitest';
import { createLucideHostProvider } from './index';

class FakeSvgElement {
  readonly attributes = new Map<string, string>();
  readonly children: FakeSvgElement[] = [];
  removed = false;

  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }

  removeAttribute(name: string) {
    this.attributes.delete(name);
  }

  append(child: FakeSvgElement) {
    this.children.push(child);
  }

  replaceChildren(...children: FakeSvgElement[]) {
    this.children.splice(0, this.children.length, ...children);
  }

  remove() {
    this.removed = true;
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('createLucideHostProvider', () => {
  it('registers only supplied icons and preserves their lifecycle', () => {
    vi.stubGlobal('document', {
      createElementNS: () => new FakeSvgElement(),
    });
    const provider = createLucideHostProvider({
      Mark: [['path', { d: 'M0 0' }]],
    });
    const icon = provider.Mark;
    expect(Object.keys(provider)).toEqual(['Mark']);
    expect(icon).toBeDefined();
    if (!icon) throw new Error('supplied icon must be registered');

    const boundary = new FakeSvgElement();
    const handle = icon.mount(boundary as unknown as Element, {
      className: 'size-4',
      title: 'mark',
    });
    const svg = handle.element as unknown as FakeSvgElement;
    expect(boundary.children).toEqual([svg]);
    expect(svg.attributes.get('class')).toBe('size-4');
    expect(svg.attributes.get('title')).toBe('mark');
    expect(svg.children).toHaveLength(1);

    icon.update(handle, { className: 'size-5' });
    expect(svg.attributes.get('class')).toBe('size-5');
    expect(svg.attributes.has('title')).toBe(false);

    icon.dispose(handle);
    expect(svg.removed).toBe(true);
  });
});
