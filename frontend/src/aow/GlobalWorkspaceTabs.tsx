import { useCallback, useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react';
import { t } from '../lib/i18n';
import type { TabRevealRequest, WorkspaceTab } from './WorkspaceTabs';

type GlobalTabKind = 'settings' | 'inbox' | 'browser';
export interface GlobalTabHost extends WorkspaceTab {
  kind: GlobalTabKind;
  workspace: string;
  element: HTMLDivElement;
  request: TabRevealRequest;
  close: () => void;
}
const definitions: { kind: GlobalTabKind; id: string; label: string }[] = [
  { kind: 'settings', id: 'settings', label: t('navigation.settings') },
  { kind: 'inbox', id: 'inbox', label: t('navigation.inbox') },
  { kind: 'browser', id: 'system-files', label: t('aow.system_file_browser') },
];

export function useGlobalWorkspaceTabs(settingsClose: RefObject<() => void>) {
  const [locations, setLocations] = useState<Partial<Record<GlobalTabKind, { workspace: string; request: TabRevealRequest }>>>({});
  // Stable portal containers preserve drafts, directory state and uploads across moves.
  const [elements] = useState(() => Object.fromEntries(definitions.map(tab => [tab.kind, document.createElement('div')])) as Record<GlobalTabKind, HTMLDivElement>);
  const open = useCallback((kind: GlobalTabKind, workspace: string) => {
    setLocations(current => ({ ...current, [kind]: { workspace, request: { tabId: kind === 'browser' ? 'system-files' : kind } } }));
  }, []);
  const close = useCallback((kind: GlobalTabKind) => {
    setLocations(current => { const next = { ...current }; delete next[kind]; return next; });
  }, []);
  const tabs = useMemo<GlobalTabHost[]>(() => definitions.flatMap(tab => {
    const location = locations[tab.kind];
    return location ? [{ ...tab, ...location, targetId: tab.id, element: elements[tab.kind],
      close: () => tab.kind === 'settings' ? settingsClose.current() : close(tab.kind) }] : [];
  }), [locations, elements, settingsClose, close]);
  const adoptWorkspace = useCallback((workspace: string, available: string[]) => {
    setLocations(current => {
      const stranded = definitions.filter(tab => current[tab.kind] && current[tab.kind]!.workspace !== workspace && !available.includes(current[tab.kind]!.workspace));
      if (!stranded.length) return current;
      const next = { ...current };
      for (const tab of stranded) next[tab.kind] = { workspace, request: { tabId: tab.id } };
      return next;
    });
  }, []);
  return { tabs, elements, open, close, adoptWorkspace };
}

export function GlobalTabSlot({ tab, hidden }: { tab: GlobalTabHost; hidden: boolean }) {
  const slot = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const container = slot.current;
    container?.appendChild(tab.element);
    return () => { if (tab.element.parentElement === container) tab.element.remove(); };
  }, [tab.element]);
  return <div ref={slot} className="project-aow-global-slot" hidden={hidden} />;
}
