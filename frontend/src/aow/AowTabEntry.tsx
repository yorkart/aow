import { appUrl } from '../lib/basePath';
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { parseTabUrl, resolveTabTarget, tabLocationUrl, tabTargetUrl, type ResolvedTab, type TabTarget } from './tabRoutes';
export { aowTabUrl } from './tabRoutes';

const TabLocationContext = createContext<(target?: TabTarget | string, hash?: string, visible?: boolean) => void>(() => {});
export const useAowTabLocation = () => useContext(TabLocationContext);
const TabNavigationContext = createContext<(target: TabTarget) => Promise<void>>(async () => {});
export const useAowTabNavigation = () => useContext(TabNavigationContext);
interface TerminalTabActivity {
  getCurrent: () => string | undefined;
  subscribe: (listener: () => void) => () => void;
}
const TerminalTabActivityContext = createContext<TerminalTabActivity>({ getCurrent: () => undefined, subscribe: () => () => {} });
export const useAowTerminalTabActivity = () => useContext(TerminalTabActivityContext);
function createTerminalTabActivity() {
  let current: string | undefined;
  const listeners = new Set<() => void>();
  return {
    getCurrent: () => current,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    update(tabId?: string) {
      if (current === tabId) return;
      current = tabId;
      for (const listener of listeners) listener();
    },
  };
}
const TabLocationErrorContext = createContext<(error: string) => void>(() => {});
export const useAowTabLocationError = () => useContext(TabLocationErrorContext);
const locationKey = () => window.location.pathname + window.location.search;

// Keep mounted workspaces alive during history navigation. Only the requested
// resource may acknowledge a pending navigation and take ownership of the URL.
export function AowTabEntry({ children }: { children: (entry?: ResolvedTab) => ReactNode }) {
  const [request, setRequest] = useState(() => ({ url: window.location.href }));
  const [entry, setEntry] = useState<ResolvedTab>();
  const [terminalTabActivity] = useState(createTerminalTabActivity);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(() => window.location.pathname.startsWith(appUrl('/aow/tabs/')));
  const [opened, setOpened] = useState(!loading);
  const pending = useRef<string | undefined>(undefined);
  const currentLocation = useRef(locationKey());
  const navigation = useRef<AbortController | undefined>(undefined);
  useEffect(() => {
    const navigate = () => {
      if (currentLocation.current === locationKey()) return;
      currentLocation.current = locationKey();
      pending.current = 'loading';
      setRequest({ url: window.location.href });
    };
    window.addEventListener('popstate', navigate);
    return () => window.removeEventListener('popstate', navigate);
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    navigation.current = controller;
    setEntry(undefined);
    setError('');
    pending.current = 'loading';
    void Promise.resolve().then(async () => {
      const target = parseTabUrl(new URL(request.url));
      if (!target) {
        pending.current = undefined;
        setLoading(false); setOpened(true);
        return;
      }
      setLoading(true);
      const resolved = await resolveTabTarget(target, controller.signal);
      if (controller.signal.aborted) return;
      pending.current = tabTargetUrl(resolved.target);
      setEntry(resolved); setLoading(false); setOpened(true);
    }).catch(reason => {
      if (!controller.signal.aborted) { setError(reason instanceof Error ? reason.message : String(reason)); setLoading(false); }
    });
    return () => navigation.current?.abort();
  }, [request]);
  const navigateTab = useCallback(async (target: TabTarget) => {
    navigation.current?.abort();
    const controller = new AbortController();
    navigation.current = controller;
    pending.current = 'loading';
    setEntry(undefined);
    try {
      // Resolve in place: keep the current workspace visible and let the caller
      // display failures without replacing the page or changing its URL.
      const resolved = await resolveTabTarget(target, controller.signal);
      if (controller.signal.aborted) return;
      pending.current = tabTargetUrl(resolved.target);
      setError(''); setLoading(false); setOpened(true); setEntry(resolved);
    } catch (reason) {
      if (controller.signal.aborted) return;
      pending.current = undefined;
      throw reason;
    }
  }, []);
  const syncTab = useCallback((value?: TabTarget | string, hash = window.location.hash, visible = true) => {
    const target: TabTarget | undefined = typeof value === 'string' ? { type: 'terminal', tabId: value } : value;
    if (pending.current && (!target || pending.current !== tabTargetUrl(target))) return;
    pending.current = undefined;
    // Publish synchronously so an arriving stop observes the newly activated tab.
    terminalTabActivity.update(visible && target?.type === 'terminal' ? target.tabId : undefined);
    const url = tabLocationUrl(target, hash);
    if (url !== locationKey() + window.location.hash) window.history.replaceState(window.history.state, '', url);
    currentLocation.current = locationKey();
    setEntry(current => current ? undefined : current);
  }, [terminalTabActivity]);
  const blocked = loading || !!error;
  useEffect(() => { if (blocked) terminalTabActivity.update(); }, [blocked, terminalTabActivity]);
  return <TabNavigationContext.Provider value={navigateTab}><TabLocationContext.Provider value={syncTab}><TabLocationErrorContext.Provider value={setError}>
    <TerminalTabActivityContext.Provider value={terminalTabActivity}>
      <div style={{ display: blocked ? 'none' : 'contents' }}>{opened && children(entry)}</div>
    </TerminalTabActivityContext.Provider>
    {blocked && <main className="aow-auth"><section className="aow-auth-card">
      <h1>{error ? '无法打开 Tab' : '正在定位 Tab…'}</h1>
      {error ? <><p role="alert">{error}</p><button onClick={() => setRequest({ url: window.location.href })}>重试</button></> : <p role="status">正在读取内容所在的工作区。</p>}
      <p><a href={tabLocationUrl(undefined)}>返回工作台</a></p>
    </section></main>}
  </TabLocationErrorContext.Provider></TabLocationContext.Provider></TabNavigationContext.Provider>;
}
