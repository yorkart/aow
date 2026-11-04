import { useCallback, useEffect, useRef, useState } from 'react';
import { tasksApi, tasksChanged, type WorkTask } from './api';

export function useTasks() {
  const [tasks, setTasks] = useState<WorkTask[]>([]);
  const [error, setError] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [revision, setRevision] = useState(0);
  const reload = useCallback(() => setRevision(value => value + 1), []);
  const version = useRef(0);
  useEffect(() => {
    const controller = new AbortController();
    const refresh = async () => {
      const request = ++version.current;
      try {
        const next = await tasksApi.list(controller.signal);
        if (!Array.isArray(next)) throw new Error('Invalid task list response');
        if (!controller.signal.aborted && request === version.current) { setTasks(next); setError(''); setLoaded(true); }
      } catch (reason) {
        if (!controller.signal.aborted && request === version.current) { setError(reason instanceof Error ? reason.message : String(reason)); setLoaded(true); }
      }
    };
    void refresh();
    const changed = () => { void refresh(); };
    const interval = window.setInterval(() => { if (document.visibilityState === 'visible') changed(); }, 5000);
    window.addEventListener(tasksChanged, changed);
    window.addEventListener('focus', changed);
    return () => { controller.abort(); clearInterval(interval); window.removeEventListener(tasksChanged, changed); window.removeEventListener('focus', changed); };
  }, [revision]);
  const accept = useCallback((task: WorkTask) => {
    ++version.current;
    setTasks(items => [...items.filter(item => item.id !== task.id), task]);
    reload();
  }, [reload]);
  return { tasks, loaded, error, reload, accept };
}
