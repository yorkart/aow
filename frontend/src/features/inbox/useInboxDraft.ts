import { useEffect, useRef, useState } from 'react';
import { inboxApi } from './api';
import { readDraft, saveDraft } from './drafts';
import { newInboxId } from './id';
import { inboxError, type InboxItem } from './types';

interface Draft {
  base?: InboxItem;
  markdown: string;
  project_id: string | null;
  label_ids: string[];
  requestKey?: string;
  captureMarkdown?: string;
  remove?: boolean;
}
type Fields = Pick<Draft, 'markdown' | 'project_id' | 'label_ids'>;
const fields = (item?: Fields): Fields => ({ markdown: item?.markdown ?? '', project_id: item?.project_id ?? null, label_ids: item?.label_ids ?? [] });
const dirty = (draft: Draft) => !draft.base || draft.markdown !== draft.base.markdown || draft.project_id !== draft.base.project_id
  || JSON.stringify(draft.label_ids) !== JSON.stringify(draft.base.label_ids);
const empty = (markdown: string) => /^[\s#]*$/.test(markdown);

// Each editor has one write in flight. Its response advances the base revision,
// while any text typed during that request remains the next pending write.
export function useInboxDraft(key: string, mutate: <T>(action: () => Promise<T>) => Promise<T>, item?: InboxItem, items?: InboxItem[]) {
  const [draft, setDraft] = useState<Draft | undefined>(() => {
    const stored = readDraft<Draft>(key);
    if (!stored || typeof stored.markdown !== 'string' || (key !== 'new' && stored.base?.id !== key)) return;
    return { ...fields(stored.base), ...stored, requestKey: stored.requestKey ?? (key === 'new' ? newInboxId() : undefined) };
  });
  const current = useRef(draft);
  const [editing, setEditing] = useState(!!draft && !draft.remove);
  const editingRef = useRef(editing);
  const showEditor = (value: boolean) => { editingRef.current = value; setEditing(value); };
  const latestItem = useRef(item); latestItem.current = item ?? items?.find(value => value.id === draft?.base?.id);
  const pending = useRef<Promise<boolean> | undefined>(undefined);
  const finishing = useRef<Promise<boolean> | undefined>(undefined);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const composing = useRef(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const put = (next?: Draft) => { current.current = next; setDraft(next); saveDraft(key, next); };
  const stopTimer = () => { clearTimeout(timer.current); timer.current = undefined; };
  const conflict = () => !!current.current?.base && !!latestItem.current && latestItem.current.revision > current.current.base.revision;
  const flush = async (): Promise<boolean> => {
    stopTimer();
    if (pending.current) return pending.current;
    if (composing.current) return false;
    const write = async () => {
      setError('');
      try {
        while (current.current && (dirty(current.current) || current.current.remove)) {
          if (conflict()) return false;
          if (composing.current) return false;
          let snapshot = current.current;
          if (snapshot.remove && (snapshot.base || snapshot.captureMarkdown === undefined)) {
            if (snapshot.base) {
              const base = snapshot.base;
              setSaving(true);
              await mutate(async () => {
                try { await inboxApi.remove(base); }
                catch (reason) {
                  // A lost DELETE response must not leave a permanent retry:
                  // a fresh snapshot can confirm that the item is already gone.
                  if ((await inboxApi.list()).items.some(item => item.id === base.id)) throw reason;
                }
              });
            }
            put();
            return true;
          }
          // While typing, keep empty content locally. Only finishing editing
          // requests removal. An uncertain capture must first recover its ID
          // with the original key/payload, then take the removal branch above.
          if (empty(snapshot.markdown) && !snapshot.remove) return true;
          setSaving(true);
          if (!snapshot.base && snapshot.captureMarkdown === undefined) {
            snapshot = { ...snapshot, captureMarkdown: snapshot.markdown };
            // Retrying a capture must use both the same key and the same payload.
            put(snapshot);
          }
          await mutate(async () => {
            const saved = snapshot.base
              ? await inboxApi.update(snapshot.base, fields(snapshot))
              : await inboxApi.capture(snapshot.captureMarkdown!, snapshot.requestKey!);
            if (current.current) put({ ...current.current, base: saved });
            return saved;
          });
        }
        if (!editingRef.current) put();
        return true;
      } catch (reason) { setError(inboxError(reason)); return false; }
      finally { setSaving(false); }
    };
    const request = write();
    pending.current = request;
    try { return await request; } finally { pending.current = undefined; }
  };
  const schedule = () => {
    stopTimer();
    if (!composing.current && current.current && (current.current.remove || (!empty(current.current.markdown) && dirty(current.current)))) {
      timer.current = setTimeout(() => { void flush(); }, 350);
    }
  };
  const begin = () => {
    if (current.current?.remove) return;
    if (!current.current) {
      const base = key === 'new' ? undefined : latestItem.current;
      put({ base, ...fields(base), markdown: base?.markdown ?? '## ', requestKey: key === 'new' ? newInboxId() : undefined });
    }
    showEditor(true); setError('');
  };
  const update = (patch: Partial<Fields>) => {
    if (!current.current || current.current.remove) return;
    put({ ...current.current, ...patch }); setError(''); schedule();
  };
  const finish = (): Promise<boolean> => {
    if (!current.current) return Promise.resolve(true);
    if (composing.current) return Promise.resolve(false);
    if (empty(current.current.markdown)) put({ ...current.current, remove: true });
    showEditor(false);
    if (finishing.current) return finishing.current;
    const request = flush();
    finishing.current = request;
    void request.finally(() => { finishing.current = undefined; });
    return request;
  };
  const resolve = (keep: boolean) => {
    if (!latestItem.current || !current.current || saving) return;
    put(keep ? { ...current.current, base: latestItem.current } : { base: latestItem.current, ...fields(latestItem.current) });
    setError('');
    if (!editingRef.current) void flush(); else schedule();
  };
  useEffect(() => {
    schedule();
    const online = () => { void flush(); };
    window.addEventListener('online', online);
    return () => { stopTimer(); window.removeEventListener('online', online); };
  // The ref owns the current draft, including changes made while a write is pending.
  }, []);
  return { draft, editing, saving, removing: !!draft?.remove, error, conflict: conflict(),
    status: draft?.remove ? (error || conflict() ? '移除失败' : '移除中…') : saving ? '保存中…' : error || conflict() ? '未保存' : draft && empty(draft.markdown) ? '编辑中' : draft && dirty(draft) ? '等待保存' : '已保存',
    begin, update, finish, resolve, retry: flush,
    setComposing: (value: boolean) => { composing.current = value; if (value) stopTimer(); else schedule(); } };
}
