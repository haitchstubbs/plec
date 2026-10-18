import { expectTypeOf, it } from 'vitest';
import type { PlecMutation } from './use-mutation.js';

it('defaults mutation errors to native Error with an optional action code', () => {
  type Mutation = PlecMutation<string, number>;

  expectTypeOf<Mutation['error']>().toEqualTypeOf<
    (Error & { code?: string }) | null
  >();
  expectTypeOf<Mutation['run']>().returns.toEqualTypeOf<
    Promise<number>
  >();
});
