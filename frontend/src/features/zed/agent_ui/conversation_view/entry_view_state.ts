// Ports the ACP subset of entry_view_state.rs; DOM elements replace GPUI entities.
import { useEffect, useState } from 'react';
import { parse } from 'jsonc-parser';
import { acpApi, settingsChanged } from '../../api';
import { list, object, text, type Data, type ThreadEntry } from '../../types';

export function visibleContent(content: Data): boolean {
  if (content.type === 'text') return !!text(content.text).trim();
  if (content.type === 'image') return !!content.data;
  if (content.type === 'resource') return !!text(object(content.resource).uri);
  return content.type === 'resource_link' && !!text(content.uri);
}
export function visibleTool(content: Data) {
  return !['cancelled', 'canceled'].includes(text(content.status)) || list(content.content).some(part => part.type === 'content' ? visibleContent(object(part.content)) : ['diff', 'terminal'].includes(text(part.type)));
}
export function messageGroups(entries: ThreadEntry[]) {
  const groups: { id: string; kind: string; entries: ThreadEntry[] }[] = [];
  for (const entry of entries) {
    if (entry.kind === 'tool' && !visibleTool(entry.content)) continue;
    if (['assistant', 'thought', 'user'].includes(entry.kind) && !visibleContent(entry.content)) continue;
    if (!['assistant', 'thought', 'user', 'tool', 'notice', 'summary', 'compaction_update'].includes(entry.kind)) continue;
    const kind = entry.kind === 'thought' ? 'assistant' : entry.kind;
    const last = groups.at(-1);
    if (last && last.kind === kind && ['assistant', 'user'].includes(kind)) last.entries.push(entry);
    else groups.push({ id: entry.id, kind, entries: [entry] });
  }
  return groups;
}
type ThinkingDisplay = 'auto' | 'preview' | 'always_expanded' | 'always_collapsed';
export function useEntryViewState(entries: ThreadEntry[], working: boolean) {
  const [display, setDisplay] = useState<ThinkingDisplay>('auto');
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [toggled, setToggled] = useState<Set<string>>(new Set());
  const [auto, setAuto] = useState<string>();
  useEffect(() => {
    let cancelled = false;
    const refresh = () => { void acpApi.settings().then(file => {
      const value = text(object(object(parse(file.content)).agent).thinking_display);
      if (!cancelled) setDisplay(['auto', 'preview', 'always_expanded', 'always_collapsed'].includes(value) ? value as ThinkingDisplay : 'auto');
    }).catch(() => { /* Default to Zed's auto setting when preferences are unavailable. */ }); };
    refresh(); window.addEventListener(settingsChanged, refresh);
    return () => { cancelled = true; window.removeEventListener(settingsChanged, refresh); };
  }, []);
  const last = entries.at(-1);
  useEffect(() => {
    if (!['auto', 'preview'].includes(display)) return;
    if (last?.kind === 'thought' && working && auto !== last.id) { setAuto(last.id); setExpanded(values => new Set([...values, last.id])); }
    else if (auto && last?.id !== auto) {
      if (display === 'auto' && !toggled.has(auto)) setExpanded(values => { const next = new Set(values); next.delete(auto); return next; });
      setAuto(undefined);
    }
  }, [last?.id, last?.kind, working, display, auto, toggled]);
  const thinking = (id: string) => ({
    open: display === 'always_expanded' ? !toggled.has(id) : display === 'always_collapsed' ? toggled.has(id) : toggled.has(id) || expanded.has(id),
    constrained: display === 'preview' && expanded.has(id) && !toggled.has(id),
    following: auto === id,
  });
  const toggleThinking = (id: string) => {
    const state = thinking(id);
    const pin = display === 'always_expanded' ? !toggled.has(id) : display === 'preview' && state.constrained || !state.open;
    setToggled(values => { const next = new Set(values); if (pin) next.add(id); else next.delete(id); return next; });
    setExpanded(values => { const next = new Set(values); if (pin) next.add(id); else next.delete(id); return next; });
  };
  return { thinking, toggleThinking };
}
