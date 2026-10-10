import { useCallback, useEffect, useRef, useState } from 'react';
import type { SessionInfo } from '../types';
import { discardComposerDraft } from './composer_drafts';

export interface AcpTab { id: string; agentId: string; sessionId?: string; title: string }

export function useAcpTabs(workspace: string) {
  const key = `aow-acp-tabs:${workspace}`;
  const [tabs, setTabs] = useState<AcpTab[]>(() => {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(key) || '[]');
      return Array.isArray(saved) ? saved.filter((tab): tab is AcpTab => tab && typeof tab.id === 'string' && typeof tab.sessionId === 'string' && typeof tab.agentId === 'string' && typeof tab.title === 'string') : [];
    } catch { return []; }
  });
  const current = useRef(tabs);
  current.current = tabs;
  const update = useCallback((next: AcpTab[]) => {
    for (const tab of current.current) if (!next.some(item => item.id === tab.id)) discardComposerDraft(workspace, tab.id);
    current.current = next; setTabs(next);
  }, [workspace]);
  useEffect(() => {
    try { localStorage.setItem(key, JSON.stringify(tabs.filter(tab => tab.sessionId))); } catch { /* View persistence can be unavailable. */ }
  }, [key, tabs]);
  const create = useCallback((agentId: string) => {
    // getRandomValues is available on LAN HTTP origins as well as secure contexts.
    const id = Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join('');
    const tab = { id: `new:${id}`, agentId, title: `新会话 · ${agentId}` };
    update([...current.current, tab]); return tab.id;
  }, [update]);
  const open = useCallback((session: SessionInfo) => {
    const existing = current.current.find(tab => tab.sessionId === session.id);
    if (existing) return existing.id;
    const tab = { id: session.id, sessionId: session.id, agentId: session.agent_id, title: session.title };
    update([...current.current, tab]); return tab.id;
  }, [update]);
  const close = useCallback((ids: string[]) => update(current.current.filter(tab => !ids.includes(tab.id))), [update]);
  const removed = useCallback((sessionId: string) => update(current.current.filter(tab => tab.sessionId !== sessionId)), [update]);
  const changed = useCallback((id: string, session: SessionInfo) => {
    const tab = current.current.find(item => item.id === id);
    if (!tab || tab.sessionId === session.id && tab.title === session.title) return;
    update(current.current.map(item => item.id === id ? { ...item, sessionId: session.id, title: session.title } : item));
  }, [update]);
  return { tabs, create, open, close, removed, changed };
}
