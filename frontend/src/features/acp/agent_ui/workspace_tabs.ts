import { useCallback, useEffect, useRef, useState } from 'react';
import type { SessionInfo } from '../types';
import { discardComposerDraft } from './composer_drafts';

export interface AcpTab { id: string; agentId: string; thread_id?: string; title: string; working?: boolean }

const removalListeners = new Set<(workspace: string, threadId: string) => void>();

export function savedAcpTabs(workspace: string): AcpTab[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(`aow-acp-tabs:${workspace}`) || '[]');
    if (!Array.isArray(saved)) return [];
    return saved.map(tab => tab && ({ id: tab.id, thread_id: tab.thread_id, agentId: tab.agentId, title: tab.title }))
      .filter((tab): tab is AcpTab => tab && typeof tab.id === 'string' && typeof tab.thread_id === 'string' && typeof tab.agentId === 'string' && typeof tab.title === 'string');
  } catch { return []; }
}

function saveAcpTabs(workspace: string, tabs: AcpTab[]) {
  try { localStorage.setItem(`aow-acp-tabs:${workspace}`, JSON.stringify(tabs.filter(tab => tab.thread_id).map(({ id, thread_id, agentId, title }) => ({ id, thread_id, agentId, title })))); } catch { /* View persistence can be unavailable. */ }
}

// Deletion can originate in another worktree's panel while the owner is unmounted.
export function removeAcpSessionTabs(workspace: string, threadId: string) {
  const saved = savedAcpTabs(workspace);
  const removed = saved.filter(tab => tab.thread_id === threadId);
  if (removed.length) {
    for (const tab of removed) discardComposerDraft(workspace, tab.id);
    saveAcpTabs(workspace, saved.filter(tab => tab.thread_id !== threadId));
  }
  for (const listener of removalListeners) listener(workspace, threadId);
}

export function useAcpTabs(workspace: string) {
  const [tabs, setTabs] = useState<AcpTab[]>(() => savedAcpTabs(workspace));
  const current = useRef(tabs);
  current.current = tabs;
  const update = useCallback((next: AcpTab[]) => {
    for (const tab of current.current) if (!next.some(item => item.id === tab.id)) discardComposerDraft(workspace, tab.id);
    saveAcpTabs(workspace, next);
    current.current = next; setTabs(next);
  }, [workspace]);
  useEffect(() => {
    const removed = (owner: string, threadId: string) => {
      if (owner === workspace && current.current.some(tab => tab.thread_id === threadId)) {
        update(current.current.filter(tab => tab.thread_id !== threadId));
      }
    };
    removalListeners.add(removed);
    return () => { removalListeners.delete(removed); };
  }, [workspace, update]);
  const create = useCallback((agentId: string) => {
    // getRandomValues is available on LAN HTTP origins as well as secure contexts.
    const id = Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join('');
    const tab = { id: `new:${id}`, agentId, title: `新会话 · ${agentId}` };
    update([...current.current, tab]); return tab.id;
  }, [update]);
  const open = useCallback((session: SessionInfo) => {
    const existing = current.current.find(tab => tab.thread_id === session.thread_id);
    if (existing) return existing.id;
    const tab = { id: session.thread_id, thread_id: session.thread_id, agentId: session.agent_id, title: session.title, working: session.status === 'working' };
    update([...current.current, tab]); return tab.id;
  }, [update]);
  const close = useCallback((ids: string[]) => update(current.current.filter(tab => !ids.includes(tab.id))), [update]);
  const changed = useCallback((id: string, session: SessionInfo) => {
    const tab = current.current.find(item => item.id === id);
    const working = session.status === 'working';
    if (!tab || tab.thread_id === session.thread_id && tab.title === session.title && tab.working === working) return;
    update(current.current.map(item => item.id === id ? { ...item, thread_id: session.thread_id, title: session.title, working } : item));
  }, [update]);
  return { tabs, create, open, close, changed };
}
