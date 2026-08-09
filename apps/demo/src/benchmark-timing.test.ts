import { describe, expect, it } from 'vitest'
import { waitForCondition } from './benchmark-timing'

describe('waitForCondition', () => {
  it('cleans up the subscription after a deferred condition becomes true', async () => {
    let ready = false
    let notify = () => {}
    let cleanedUp = false
    const pending = waitForCondition(
      () => ready,
      (callback) => { notify = callback; return () => { cleanedUp = true } },
    )

    ready = true
    notify()
    await expect(pending).resolves.toBeUndefined()
    expect(cleanedUp).toBe(true)
  })

  it('can settle a property-only update through a polling subscription', async () => {
    let value = 'before'
    let poll = () => {}
    const pending = waitForCondition(
      () => value === 'after',
      (notify) => { poll = notify; return () => {} },
    )
    value = 'after'
    poll()
    await expect(pending).resolves.toBeUndefined()
  })
})
