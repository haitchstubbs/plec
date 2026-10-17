import { describe, expect, it } from 'vitest';
import { actionFailure } from './server.js';

describe('server action failures', () => {
  it('throws a terminal failure containing only its public code and message', () => {
    let thrown: unknown;
    try {
      actionFailure({
        code: 'EMAIL_TAKEN',
        message: 'That email address is already registered.',
      });
    } catch (error) {
      thrown = error;
    }

    expect(thrown).toMatchObject({
      code: 'EMAIL_TAKEN',
      message: 'That email address is already registered.',
    });
    expect(Object.keys(thrown as object).sort()).toEqual([
      'code',
      'message',
    ]);
  });
});
