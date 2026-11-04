import { t } from '../../lib/i18n';
import { useLayoutEffect, useRef, useState } from 'react';
import { filesApi } from './api';

interface Upload {
  id: number;
  file: File;
  directory: string;
  status: 'waiting' | 'uploading' | 'success' | 'error';
  loaded: number;
  path?: string;
  error?: string;
}

export function useExplorerUploads(root: string, onUploaded: (directory: string, path: string) => Promise<void>) {
  const [uploads, setUploads] = useState<Upload[]>([]);
  const queue = useRef<Upload[]>([]);
  const nextId = useRef(0);
  const epoch = useRef(0);
  const running = useRef(false);
  const controller = useRef<AbortController | undefined>(undefined);
  const completed = useRef(onUploaded);
  useLayoutEffect(() => { completed.current = onUploaded; });
  useLayoutEffect(() => {
    setUploads([]);
    return () => {
      epoch.current += 1;
      queue.current = [];
      controller.current?.abort();
      running.current = false;
    };
  }, [root]);

  const enqueue = (directory: string, files: File[]) => {
    if (!files.length) return;
    const items: Upload[] = files.map((file) => ({ id: ++nextId.current, file, directory, status: 'waiting', loaded: 0 }));
    queue.current.push(...items);
    setUploads((previous) => [...previous, ...items]);
    if (running.current) return;
    running.current = true;
    const generation = epoch.current;
    const update = (id: number, changes: Partial<Upload>) => {
      if (epoch.current === generation) setUploads((previous) => previous.map((item) => item.id === id ? { ...item, ...changes } : item));
    };
    void (async () => {
      try {
        while (queue.current.length && epoch.current === generation) {
          const item = queue.current.shift()!;
          const abort = new AbortController();
          controller.current = abort;
          update(item.id, { status: 'uploading' });
          try {
            if (!item.file.name || item.file.name === '.' || item.file.name === '..' || /[/\\\0]/.test(item.file.name)) {
              throw new Error(t("files.invalid_file_name_choose_the_file_again"));
            }
            const result = await filesApi.uploadFile(item.directory, item.file, (loaded) => update(item.id, { loaded }), { keepBoth: true, signal: abort.signal });
            if (epoch.current !== generation) return;
            update(item.id, { status: 'success', path: result.path, loaded: item.file.size });
            try { await completed.current(item.directory, result.path); }
            catch (reason) { update(item.id, { error: t("files.file_uploaded_but_directory_refresh_failed_2", { value0: reason instanceof Error ? reason.message : String(reason) }) }); }
          } catch (reason) {
            update(item.id, { status: 'error', error: reason instanceof Error ? reason.message : String(reason) });
          }
        }
      } finally {
        if (epoch.current === generation) { running.current = false; controller.current = undefined; }
      }
    })();
  };

  return {
    uploads, enqueue,
    cancel: () => {
      const waiting = new Set(queue.current.map((item) => item.id));
      queue.current = [];
      controller.current?.abort();
      setUploads((previous) => previous.map((item) => waiting.has(item.id) ? { ...item, status: 'error', error: t("files.upload_canceled") } : item));
    },
    clear: () => setUploads((previous) => previous.filter((item) => item.status === 'waiting' || item.status === 'uploading')),
    retry: (item: Upload) => {
      setUploads((previous) => previous.filter((previous) => previous.id !== item.id));
      enqueue(item.directory, [item.file]);
    },
  };
}
