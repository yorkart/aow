import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { EditorSettings, OpenDocument } from './types';
import { aowApi } from '../../aow/aowApi';

export const defaultEditorSettings: EditorSettings = { word_wrap: false };
const EditorSettingsContext = createContext({
  editor: defaultEditorSettings,
  updateEditorSettings: (_editor: EditorSettings) => {},
});

export function EditorSettingsProvider({ children }: { children: ReactNode }) {
  const [editor, setEditor] = useState(defaultEditorSettings);
  const revision = useRef(0);
  const updateEditorSettings = useCallback((next: EditorSettings) => {
    revision.current += 1;
    setEditor(next);
  }, []);
  useEffect(() => {
    let active = true;
    const started = revision.current;
    void aowApi.settings().then(settings => {
      // A later Settings load/save takes precedence over the initial request.
      if (active && revision.current === started) setEditor(settings.editor ?? defaultEditorSettings);
    }).catch(() => {});
    return () => { active = false; };
  }, []);
  const value = useMemo(() => ({ editor, updateEditorSettings }), [editor, updateEditorSettings]);
  return <EditorSettingsContext.Provider value={value}>{children}</EditorSettingsContext.Provider>;
}

export const useEditorSettings = () => useContext(EditorSettingsContext);

// Keep view preferences outside shared document content and persisted tab data.
export function useWordWrapOverrides(documents: OpenDocument[]) {
  const [overrides, setOverrides] = useState(new Map<number | string, boolean>());
  const keyFor = (id: string) => {
    const document = documents.find(document => document.id === id);
    return document ? document.instanceId ?? document.id : undefined;
  };
  useEffect(() => {
    const keys = new Set(documents.map(document => document.instanceId ?? document.id));
    setOverrides(current => [...current.keys()].some(key => !keys.has(key))
      ? new Map([...current].filter(([key]) => keys.has(key))) : current);
  }, [documents]);
  return {
    wordWrapFor: (id?: string) => {
      const key = id === undefined ? undefined : keyFor(id);
      return key === undefined ? undefined : overrides.get(key);
    },
    setWordWrap: (id: string, enabled: boolean) => {
      const key = keyFor(id);
      if (key !== undefined) setOverrides(current => new Map(current).set(key, enabled));
    },
  };
}
