import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Folder, RefreshCw } from 'lucide-react';
import { AowPanel, AowPanelStack } from '../src/components/AowPanel';
import { AowIconButton } from '../src/components/AowIconButton';
import '../src/styles.css';

function Preview() {
  const nested = new URLSearchParams(location.search).has('nested');
  const [short, setShort] = useState(false);
  const [filled, setFilled] = useState(false);
  const [many, setMany] = useState(false);
  const [refreshes, setRefreshes] = useState(0);
  const rows = (count: number) => Array.from({ length: count }, (_, index) => <div key={index} style={{ height: 24 }}>Row {index}</div>);
  const panels = <>
    <AowPanel title="Alpha" icon={<Folder />} actions={<AowIconButton aria-label="Refresh Alpha" onClick={() => setRefreshes(value => value + 1)}><RefreshCw /></AowIconButton>}>
      <input aria-label="Preserved input" defaultValue="initial" style={{ height: 24 }} />
      {rows(short ? 1 : 36)}
    </AowPanel>
    <AowPanel title="Beta" icon={<Folder />}>{rows(short ? 2 : 30)}</AowPanel>
    <AowPanel title="Gamma" icon={<Folder />}>{rows(2)}</AowPanel>
    <AowPanel title="Empty" icon={<Folder />} empty={!filled}><div>{filled ? 'New item' : 'No items'}</div></AowPanel>
    {many ? Array.from({ length: 24 }, (_, index) => <AowPanel key={index} title={`Project ${index}`} icon={<Folder />}>
      <AowPanel title={`Nested ${index}`} icon={<Folder />}>{rows(2)}</AowPanel>
    </AowPanel>) : null}
  </>;
  return <div className="project-aow" style={{ display: 'block' }}>
    <div style={{ height: 40 }}>
      <button onClick={() => setShort(value => !value)}>Toggle short content</button>
      <button onClick={() => setFilled(value => !value)}>Toggle empty data</button>
      <button onClick={() => setMany(value => !value)}>Toggle many panels</button>
      <output aria-label="Refresh count">{refreshes}</output>
    </div>
    <div style={{ width: 320, height: 'calc(100vh - 40px)' }}>
      <AowPanelStack>{nested ? <AowPanel title="Parent" icon={<Folder />}>{panels}</AowPanel> : panels}</AowPanelStack>
    </div>
  </div>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>);
