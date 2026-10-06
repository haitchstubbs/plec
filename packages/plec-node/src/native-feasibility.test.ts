import { createRequire } from 'node:module';
import { describe, expect, it } from 'vitest';

const native = createRequire(import.meta.url)(
  new URL('../native/index.linux-x64-gnu.node', import.meta.url)
    .pathname,
) as {
  FeasibilityApplication: new (
    callback: (value: string) => Promise<string>,
  ) => {
    invoke(value: string): Promise<string>;
    close(): void;
  };
  consumeStream(
    stream: ReadableStream<Uint8Array>,
    maxBytes: number,
    cancelSource: (reason: string) => Promise<void>,
  ): Promise<number>;
  produceStream(): ReadableStream<Uint8Array>;
  produceFailingStream(): ReadableStream<Uint8Array>;
  outboundPollCount(): number;
};

describe('napi-rs feasibility gate (Node 22)', () => {
  it('loads the native class and supports Promise callback re-entry, rejection, and close', async () => {
    let application!: InstanceType<typeof native.FeasibilityApplication>;
    application = new native.FeasibilityApplication(async (value) => {
      if (value === 'outer')
        return `outer:${await application.invoke('inner')}`;
      if (value === 'reject')
        throw new Error('expected callback rejection');
      if (value === 'pending')
        return await new Promise<string>((resolve) => {
          releasePending = resolve;
        });
      return value;
    });
    let releasePending!: (value: string) => void;
    try {
      await expect(application.invoke('outer')).resolves.toBe(
        'outer:inner',
      );
      await expect(application.invoke('reject')).rejects.toThrow(
        'JavaScript callback failed',
      );

      const pending = application.invoke('pending');
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
      application.close();
      await expect(application.invoke('late')).rejects.toThrow(
        'PLEC_APPLICATION_CLOSED',
      );
      releasePending('settled after close');
      await expect(pending).resolves.toBe('settled after close');
      application.close();
    } finally {
      application.close();
    }
  });

  it('consumes multi-chunk Web streams and streams bytes on demand', async () => {
    const chunks = new ReadableStream<Uint8Array>(
      {
        start(controller) {
          controller.enqueue(new Uint8Array([1, 2]));
          controller.enqueue(new Uint8Array([3]));
          controller.close();
        },
      },
      { highWaterMark: 0 },
    );
    await expect(
      native.consumeStream(chunks, 3, async () => {}),
    ).resolves.toBe(3);

    const before = native.outboundPollCount();
    const output = native.produceStream();
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(native.outboundPollCount() - before).toBeLessThan(3);
    const reader = output.getReader();
    const body: number[] = [];
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      body.push(...value);
    }
    expect(new TextDecoder().decode(Uint8Array.from(body))).toBe(
      'plec!',
    );
    expect(native.outboundPollCount() - before).toBe(3);
  });

  it('propagates native response stream errors to the Web reader', async () => {
    const reader = native.produceFailingStream().getReader();
    await expect(reader.read()).resolves.toMatchObject({ done: false });
    await expect(reader.read()).rejects.toThrow(
      'native stream failure',
    );
  });

  it('stops at the limit and invokes the explicit source-cancellation bridge', async () => {
    let pulls = 0;
    let canceled = false;
    const source = new ReadableStream<Uint8Array>(
      {
        pull(controller) {
          pulls += 1;
          controller.enqueue(new Uint8Array([pulls]));
        },
        cancel() {
          canceled = true;
        },
      },
      { highWaterMark: 0 },
    );
    const sourceReader = source.getReader();
    const nativeInput = new ReadableStream<Uint8Array>(
      {
        async pull(controller) {
          const result = await sourceReader.read();
          if (result.done) controller.close();
          else controller.enqueue(result.value);
        },
      },
      { highWaterMark: 0 },
    );
    await expect(
      native.consumeStream(nativeInput, 1, async (reason) => {
        expect(reason).toBe('request body exceeds limit');
        await sourceReader.cancel(reason);
        canceled = true;
      }),
    ).rejects.toThrow('request body exceeds limit');
    expect(pulls).toBe(2);
    expect(canceled).toBe(true);
  });
});
