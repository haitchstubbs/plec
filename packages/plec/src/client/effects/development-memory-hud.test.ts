import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import { installDevelopmentMemoryHud } from './development-memory-hud';

describe('installDevelopmentMemoryHud', () => {
  let originalDocument: typeof globalThis.document;
  let originalWindow: typeof globalThis.window;
  let originalFetch: typeof globalThis.fetch;
  let appendedElements: HTMLElement[];
  let clickHandlers: Map<HTMLElement, () => void>;

  beforeEach(() => {
    originalDocument = globalThis.document;
    originalWindow = globalThis.window;
    originalFetch = globalThis.fetch;
    appendedElements = [];
    clickHandlers = new Map();

    const createElement = (tag: string) => {
      const el = {
        tagName: tag.toUpperCase(),
        type: '',
        className: '',
        textContent: '',
        hidden: false,
        innerHTML: '',
        attributes: {} as Record<string, string>,
        setAttribute(name: string, value: string) {
          this.attributes[name] = value;
        },
        addEventListener(event: string, handler: () => void) {
          if (event === 'click')
            clickHandlers.set(this as unknown as HTMLElement, handler);
        },
        removeEventListener(event: string, handler: () => void) {
          if (
            event === 'click' &&
            clickHandlers.get(this as unknown as HTMLElement) ===
              handler
          ) {
            clickHandlers.delete(this as unknown as HTMLElement);
          }
        },
        remove() {
          const index = appendedElements.indexOf(
            this as unknown as HTMLElement,
          );
          if (index !== -1) appendedElements.splice(index, 1);
        },
      };
      return el as unknown as HTMLElement;
    };

    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: {
        createElement,
        body: {
          append(...elements: HTMLElement[]) {
            appendedElements.push(...elements);
          },
        },
      },
    });

    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: {
        setInterval: vi.fn(() => 123),
        clearInterval: vi.fn(),
      },
    });
  });

  afterEach(() => {
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: originalDocument,
    });
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    });
    Object.defineProperty(globalThis, 'fetch', {
      configurable: true,
      value: originalFetch,
    });
  });

  it('renders memory statistics correctly with arrayBuffersBytes', async () => {
    const memoryPayload = {
      timestamp: new Date().toISOString(),
      pid: 1234,
      uptimeSeconds: 10,
      rssBytes: 50 * 1024 * 1024,
      heapTotalBytes: 30 * 1024 * 1024,
      heapUsedBytes: 20 * 1024 * 1024,
      externalBytes: 5 * 1024 * 1024,
      arrayBuffersBytes: 2 * 1024 * 1024,
      heapLimitBytes: 100 * 1024 * 1024,
      activeResources: ['TCPSERVERWRAP'],
    };

    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => memoryPayload,
    } as unknown as Response);

    const cleanup = installDevelopmentMemoryHud((path) => path);

    expect(appendedElements.length).toBe(2);
    const button = appendedElements[0]!;
    const panel = appendedElements[1]!;
    expect(button.className).toBe('plec-development-memory-toggle');
    expect(panel.className).toBe('plec-development-memory-panel');

    const onClick = clickHandlers.get(button);
    expect(onClick).toBeDefined();
    onClick!();

    // Wait for the async refresh to complete
    await vi.waitFor(() => {
      expect(panel.innerHTML).toContain(
        'Node external: 5 MB · Array buffers: 2 MB',
      );
    });
    expect(panel.innerHTML).not.toContain('NaN');

    cleanup();
    expect(appendedElements.length).toBe(0);
  });
});
