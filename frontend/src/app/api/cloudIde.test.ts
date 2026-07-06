import { afterEach, describe, expect, it, vi } from 'vitest'
import { CloudIdeHttpError, saveWorkspaceFileContent } from './cloudIde'

describe('cloud IDE API', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('preserves HTTP status for stale revision conflicts', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response('workspace revision changed', {
      status: 409,
    }))

    await expect(saveWorkspaceFileContent('workspace-1', 'src/main.rs', 'fn main() {}', 'rev-old', 'session-token'))
      .rejects
      .toMatchObject({
        name: 'CloudIdeHttpError',
        message: 'workspace revision changed',
        status: 409,
      } satisfies Partial<CloudIdeHttpError>)
  })
})
