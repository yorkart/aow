import { useCallback, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { SourceControl } from '../src/features/git/SourceControl';
import { usePinnedWorktrees } from '../src/aow/usePinnedWorktrees';
import '../src/styles.css';

const repository = { path: '/fixture', name: 'Fixture', branch: 'main' };

function Preview() {
  const [pins, updatePins, pinState] = usePinnedWorktrees();
  const [visible, setVisible] = useState(true);
  const [selected, setSelected] = useState<string>();
  const [, update] = useState(0);
  const renders = useRef(0);
  const opened = useRef<unknown[]>([]);
  renders.current += 1;
  const openDiff = useCallback((repo, file, staged) => {
    opened.current.push({ repo, file, staged });
    setSelected(`diff:${repo}:${staged}:${file.path}`);
  }, []);
  const openCommitDiff = useCallback((repo, commit, file) => {
    opened.current.push({ repo, commit: commit.id, file });
  }, []);
  window.performancePreview = { renders: renders.current, opened: opened.current, setVisible, update: () => update(value => value + 1), updatePins };
  return <>
    <button onClick={pinState.reload} disabled={pinState.loading}>Refresh pins</button>
    <output data-pins>{JSON.stringify([...pins])}</output>
    <output data-error>{pinState.error}</output>
    <div hidden={!visible} style={{ width: 400, height: 650 }}>
      <SourceControl root={repository.path} fixedRepository={repository} visible={visible} titleCase aowHeader
        activeDocumentId={selected} onOpenDiff={openDiff} onOpenCommitDiff={openCommitDiff} />
    </div>
  </>;
}

createRoot(document.getElementById('root')!).render(<Preview />);
