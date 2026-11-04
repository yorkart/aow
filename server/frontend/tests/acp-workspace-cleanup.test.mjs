import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

test('ACP workspace cleanup removes saved tabs only for the deleted worktree', async () => {
  const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)),
    server: { middlewareMode: true }, logLevel: 'error' });
  const storage = new Map();
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: key => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
    removeItem: key => storage.delete(key),
  } });
  try {
    const { removeAcpWorkspaceTabs, savedAcpTabs, pruneAcpWorkspaceTabs } = await server.ssrLoadModule('/src/features/acp/agent_ui/workspace_tabs.ts');
    const tabs = [{ id: 'tab', thread_id: 'thread', agentId: 'test', title: 'Conversation' }];
    storage.set('aow-acp-tabs:/repo/worktree', JSON.stringify(tabs));
    storage.set('aow-acp-tabs:/repo/worktree-other', JSON.stringify(tabs));
    removeAcpWorkspaceTabs('/repo/worktree');
    assert.equal(storage.has('aow-acp-tabs:/repo/worktree'), false);
    assert.deepEqual(savedAcpTabs('/repo/worktree'), []);
    assert.deepEqual(savedAcpTabs('/repo/worktree-other'), tabs);
    removeAcpWorkspaceTabs('/repo/worktree');
    assert.deepEqual(savedAcpTabs('/repo/worktree-other'), tabs);
    const kept = { id: 'kept', thread_id: 'kept-thread', agentId: 'test', title: 'Kept' };
    const gone = { id: 'gone', thread_id: 'gone-thread', agentId: 'test', title: 'Deleted before directory failure' };
    storage.set('aow-acp-tabs:/repo/worktree', JSON.stringify([kept, gone]));
    const checked = savedAcpTabs('/repo/worktree').map(tab => tab.thread_id);
    // A new session is created while the authoritative inventory request is pending.
    const newer = { id: 'new', thread_id: 'new-thread', agentId: 'test', title: 'Created later' };
    storage.set('aow-acp-tabs:/repo/worktree', JSON.stringify([kept, gone, newer]));
    pruneAcpWorkspaceTabs('/repo/worktree', checked, new Set(['kept-thread']));
    assert.deepEqual(savedAcpTabs('/repo/worktree'), [kept, newer]);
    assert.deepEqual(savedAcpTabs('/repo/worktree-other'), tabs);

  } finally {
    if (previous) Object.defineProperty(globalThis, 'localStorage', previous);
    else delete globalThis.localStorage;
    await server.close();
  }
});
