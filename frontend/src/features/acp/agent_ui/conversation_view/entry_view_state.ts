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
  return !['cancelled', 'canceled'].includes(text(content.status)) || toolContent(content).some(part => part.type === 'content' ? visibleContent(object(part.content)) : ['diff', 'terminal'].includes(text(part.type)));
}
// the upstream implementation's ToolCall::content prefers structured content, falling back to rawOutput.
export function toolContent(content: Data): Data[] {
  const parts = list(content.content);
  if (parts.length || content.rawOutput == null) return parts;
  return [{ type: 'content', content: { type: 'text', text: rawToolMarkdown(content.rawOutput) } }];
}
export function rawToolMarkdown(value: unknown): string {
  if (value == null) return '';
  return typeof value === 'object' ? `\`\`\`json\n${JSON.stringify(value, null, 2)}\n\`\`\`` : String(value);
}
export function contentMarkdown(content: Data): string {
  if (content.type === 'text') return text(content.text);
  if (content.type === 'image') return '`Image`';
  if (content.type === 'resource_link') return text(content.uri);
  if (content.type === 'resource') { const resource = object(content.resource); return text(resource.text) || text(resource.uri); }
  return '';
}
export interface MessageGroup { id: string; kind: string; entries: ThreadEntry[]; reply: string; showControls: boolean; userMessageId?: string }
export function visibleMessageGroups(groups: MessageGroup[], showThinking: boolean) {
  if (showThinking) return groups;
  return groups.flatMap(group => {
    const entries = group.entries.filter(entry => entry.kind !== 'thought');
    // Keep message identities stable when thoughts share a group with a reply.
    return entries.length === group.entries.length ? [group] : entries.length ? [{ ...group, entries }] : [];
  });
}

export function messageGroups(entries: ThreadEntry[], working: boolean, waiting: boolean, permissionTools = new Set<string>(), turnStarts = new Set<string>()) {
  const groups: MessageGroup[] = [];
  let userMessageId: string | undefined;
  for (const entry of entries) {
    if (entry.kind === 'tool' && !permissionTools.has(entry.id) && !visibleTool(entry.content)) continue;
    if (['assistant', 'thought', 'user'].includes(entry.kind) && !visibleContent(entry.content)) continue;
    if (!['assistant', 'thought', 'user', 'tool', 'summary', 'compaction_update'].includes(entry.kind)) continue;
    const kind = entry.kind === 'thought' ? 'assistant' : entry.kind;
    const last = groups.at(-1);
    if (last && last.kind === kind && ['assistant', 'user'].includes(kind) && !turnStarts.has(entry.id)) last.entries.push(entry);
    else {
      if (kind === 'user') userMessageId = entry.id;
      groups.push({ id: entry.id, kind, entries: [entry], reply: '', showControls: false, userMessageId });
    }
  }
  // The controls belong to the whole completed turn, including a tool-only ending.
  let turn: typeof groups = [];
  const finish = () => {
    const last = turn.at(-1);
    if (last && last.kind !== 'user') {
      last.reply = turn.flatMap(group => group.entries)
        .filter(entry => entry.kind === 'assistant').map(entry => contentMarkdown(entry.content))
        .filter(value => value.trim()).join('\n\n');
      // Bottom navigation remains available even when there is no reply to copy.
      last.showControls = !!last.reply || last === groups.at(-1);
    }
    turn = [];
  };
  for (const group of groups) {
    if (group.kind === 'user') finish();
    turn.push(group);
  }
  if (!working && !waiting) finish();
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
    }).catch(() => { /* Default to the upstream implementation's auto setting when preferences are unavailable. */ }); };
    refresh(); window.addEventListener(settingsChanged, refresh);
    return () => { cancelled = true; window.removeEventListener(settingsChanged, refresh); };
  }, []);
  let last: ThreadEntry | undefined;
  for (let index = entries.length - 1; index >= 0; index--) {
    if (!['plan', 'notice'].includes(entries[index].kind)) { last = entries[index]; break; }
  }
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
