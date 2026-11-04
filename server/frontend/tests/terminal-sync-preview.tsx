import { StrictMode, useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { useTerminals } from '../src/features/terminals/useTerminals';
import { useProjectTerminals } from '../src/features/terminals/useProjectTerminals';
import type { AowWorktree } from '../src/aow/types';

declare global {
  interface Window {
    terminalProbes: Record<string, ReturnType<typeof useTerminals>>;
    projectProbe: ReturnType<typeof useProjectTerminals>;
    setTerminalPreviewMounted: (mounted: boolean) => void;
  }
}
window.terminalProbes = {};
const worktrees = ['/one', '/two'].map(path => ({ path } as AowWorktree));

function Workspace({ path }: { path: string }) {
  const terminals = useTerminals(path, true);
  useEffect(() => { window.terminalProbes[path] = terminals; });
  return <section data-workspace={path}>
    {terminals.tabs.map(tab => <p key={tab.id}>{tab.name}</p>)}
    {terminals.error && <p role="alert">{terminals.error}</p>}
  </section>;
}
function Project() {
  const terminals = useProjectTerminals(worktrees, true);
  useEffect(() => { window.projectProbe = terminals; });
  return <aside>{terminals.tabs.map(tab => <p key={tab.id}>{tab.name}</p>)}</aside>;
}
const root = createRoot(document.body.appendChild(document.createElement('main')));
window.setTerminalPreviewMounted = mounted => root.render(mounted ? <StrictMode>
  <Workspace path="/one" /><Workspace path="/two" /><Project />
</StrictMode> : null);
window.setTerminalPreviewMounted(true);
