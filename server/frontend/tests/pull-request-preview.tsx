import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { PullRequestDetailView } from '../src/features/pr/PullRequestDetailView';
import { PullRequestsPanel } from '../src/features/pr/PullRequestsPanel';
import type { PullRequestSummary } from '../src/features/pr/types';
import '../src/styles.css';
const scenario = new URLSearchParams(location.search).get('scenario') || 'default';
const repository = '/fixtures/' + scenario;

function PanelPreview() {
  const [selected, setSelected] = useState<PullRequestSummary>();
  return <div style={{ display: 'flex', height: '100vh' }}>
    <main style={{ flex: 1, minWidth: 0, overflow: 'auto' }}>
      {selected ? <PullRequestDetailView key={selected.number} repository={repository} number={selected.number} provider={selected.provider} remote={selected.remote} visible /> : <div className="side-empty">选择一个 PR 查看详情。</div>}
    </main>
    <aside className="project-aow-right" style={{ width: 320, flexShrink: 0 }}>
      <PullRequestsPanel repository={repository} visible activeNumber={selected?.number} activeProvider={selected?.provider} activeRemote={selected?.remote} onOpen={setSelected} />
    </aside>
  </div>;
}

createRoot(document.getElementById('root')!).render(<StrictMode>{new URLSearchParams(location.search).has('panel')
  ? <PanelPreview /> : <PullRequestDetailView repository={repository} number={42} visible />}</StrictMode>);
