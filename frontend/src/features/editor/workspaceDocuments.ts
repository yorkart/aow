import { useCallback, useEffect, useSyncExternalStore } from 'react';
import type { SetStateAction } from 'react';
import { loadEditorArea } from './editorLoader';
import type { OpenDocument } from './types';

const owners = new Map<string, OpenDocument[]>();
const listeners = new Set<() => void>();
const mounts = new Map<string, number>();
const empty: OpenDocument[] = [];
let instance = 0;
export const nextDocumentInstanceId = () => ++instance;
export const savingDocuments = new Set<string>();
export const failedDocuments = new Set<string>();
export function sharedDocument(id: string) {
  for (const documents of owners.values()) {
    const found = documents.find(document => document.id === id);
    if (found) return found;
  }
}
export function updateSharedDocument(target: OpenDocument, update: (document: OpenDocument) => OpenDocument) {
  let next: OpenDocument | undefined;
  for (const [owner, documents] of owners) {
    const matches = (document: OpenDocument) => document.instanceId === target.instanceId;
    const current = documents.find(matches);
    if (!current) continue;
    next ??= update(current);
    owners.set(owner, documents.map(document => matches(document) ? next! : document));
  }
  if (next) for (const listener of listeners) listener();
}
export function isPreviewOwned(url: string) {
  return [...owners.values()].some(documents => documents.some(item => item.imageUrl === url));
}
export function hasDocumentOwners(document: OpenDocument) {
  return [...owners.values()].some(documents => documents.some(item => item.instanceId === document.instanceId && item.id === document.id));
}
export function useWorkspaceDocuments(workspace: string) {
  useEffect(() => {
    mounts.set(workspace, (mounts.get(workspace) ?? 0) + 1);
    return () => {
      mounts.set(workspace, (mounts.get(workspace) ?? 1) - 1);
      window.setTimeout(() => {
        if (mounts.get(workspace)) return;
        const documents = owners.get(workspace) ?? [];
        owners.delete(workspace);
        mounts.delete(workspace);
        if (documents.length) void loadEditorArea().then(module => module.disposeEditorModels(documents));
      }, 0);
    };
  }, [workspace]);
  const subscribe = useCallback((listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; }, []);
  const snapshot = useCallback(() => owners.get(workspace) ?? empty, [workspace]);
  const documents = useSyncExternalStore(subscribe, snapshot);
  const setDocuments = useCallback((action: SetStateAction<OpenDocument[]>) => {
    const previous = owners.get(workspace) ?? empty;
    const next = typeof action === 'function' ? action(previous) : action;
    const updates = new Map<number | undefined, OpenDocument>();
    const resolved = next.map(document => {
      const old = previous.find(item => item.instanceId === document.instanceId);
      if (old) { if (document !== old) updates.set(document.instanceId, document); return document; }
      return sharedDocument(document.id) ?? document;
    });
    owners.set(workspace, resolved);
    for (const [owner, items] of owners) {
      if (owner === workspace) continue;
      if (items.some(item => updates.has(item.instanceId))) owners.set(owner, items.map(item => updates.get(item.instanceId) ?? item));
    }
    for (const listener of listeners) listener();
  }, [workspace]);
  return [documents, setDocuments] as const;
}
