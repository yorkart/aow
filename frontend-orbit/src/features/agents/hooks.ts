import { useEffect, useRef, useState } from 'react';
export function useDirty(dirty: boolean, onChange: (dirty: boolean) => void) {
  useEffect(() => { onChange(dirty); return () => onChange(false); }, [dirty, onChange]);
  useEffect(() => {
    if (!dirty) return;
    const unload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', unload); return () => window.removeEventListener('beforeunload', unload);
  }, [dirty]);
}
export function useOperation() {
  const [busy, setBusy] = useState(false); const [error, setError] = useState(''); const lock = useRef(false);
  const perform = async (action: () => Promise<void>) => {
    if (lock.current) return; lock.current = true; setBusy(true); setError('');
    try { await action(); } catch (error) { setError(error instanceof Error ? error.message : '操作失败，请重试。'); }
    finally { lock.current = false; setBusy(false); }
  };
  return { busy, error, perform };
}
