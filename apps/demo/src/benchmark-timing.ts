export function waitForCondition(
  matches: () => boolean,
  subscribe: (notify: () => void) => () => void,
  timeoutMs = 5_000,
): Promise<void> {
  if (matches()) return Promise.resolve()

  return new Promise((resolve, reject) => {
    let settled = false
    const finish = (error?: Error) => {
      if (settled) return
      settled = true
      unsubscribe()
      clearTimeout(timeout)
      error ? reject(error) : resolve()
    }
    const notify = () => { if (matches()) finish() }
    const unsubscribe = subscribe(notify)
    const timeout = setTimeout(() => finish(new Error('Timed out waiting for the expected DOM state.')), timeoutMs)
    notify()
  })
}

export function waitForDom(root: Node, matches: () => boolean): Promise<void> {
  return waitForCondition(matches, (notify) => {
    const observer = new MutationObserver(notify)
    observer.observe(root, { attributes: true, childList: true, characterData: true, subtree: true })
    let frame = 0
    const poll = () => { notify(); frame = requestAnimationFrame(poll) }
    frame = requestAnimationFrame(poll)
    return () => { observer.disconnect(); cancelAnimationFrame(frame) }
  })
}

export function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()))
}
