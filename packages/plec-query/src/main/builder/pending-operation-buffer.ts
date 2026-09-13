import type { PendingOp } from '#types';

export class PendingOpBuffer {
  public static readonly empty = new PendingOpBuffer(
    undefined,
    undefined,
    0,
  );

  private _arrayCache: readonly PendingOp[] | undefined;

  private constructor(
    private readonly previous: PendingOpBuffer | undefined,
    private readonly op: PendingOp | undefined,
    public readonly length: number,
  ) {}

  public append(op: PendingOp): PendingOpBuffer {
    return new PendingOpBuffer(this, op, this.length + 1);
  }

  public toArray(): readonly PendingOp[] {
    if (this._arrayCache !== undefined) return this._arrayCache;
    const ops = new Array<PendingOp>(this.length);
    let currentOp = this.op;
    let previous = this.previous;

    for (let index = this.length - 1; index >= 0; index -= 1) {
      if (currentOp === undefined) {
        throw new Error('Pending op buffer is missing an operation.');
      }
      ops[index] = currentOp;
      currentOp = previous?.op;
      previous = previous?.previous;
    }

    this._arrayCache = ops;
    return ops;
  }

  public some(predicate: (op: PendingOp) => boolean): boolean {
    let currentOp = this.op;
    let previous = this.previous;
    while (currentOp !== undefined) {
      if (predicate(currentOp)) return true;
      currentOp = previous?.op;
      previous = previous?.previous;
    }
    return false;
  }

  public findLast(
    predicate: (op: PendingOp) => boolean,
  ): PendingOp | undefined {
    let currentOp = this.op;
    let previous = this.previous;
    while (currentOp !== undefined) {
      if (predicate(currentOp)) return currentOp;
      currentOp = previous?.op;
      previous = previous?.previous;
    }
    return undefined;
  }
}
