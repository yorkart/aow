import { useEffect, useRef, useState } from 'react';
import { ArrowUp, Folder, X } from 'lucide-react';
import { browseDirectories } from './api';
import type { DirectoryList } from './types';

export function DirectoryPicker({ org, initialPath, onSelect, onClose }: { org: string; initialPath: string; onSelect: (path: string) => void; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null); const [path, setPath] = useState(initialPath); const [target, setTarget] = useState(initialPath); const [result, setResult] = useState<DirectoryList>(); const [error, setError] = useState(''); const [retry, setRetry] = useState(0);
  useEffect(() => { const node = dialog.current!; node.showModal(); return () => node.close(); }, []);
  useEffect(() => { const abort = new AbortController(); void browseDirectories(org, target, abort.signal).then(value => { if (!abort.signal.aborted) { setResult(value); setPath(value.directory); setError(''); } }).catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '无法读取目录。'); }); return () => abort.abort(); }, [org, target, retry]);
  const open = (path: string) => { setResult(undefined); setError(''); setTarget(path); setRetry(value => value + 1); };
  return <dialog ref={dialog} className="project-directory-dialog" aria-labelledby="directory-title" onCancel={onClose}><header><h2 id="directory-title">选择本地目录</h2><button type="button" aria-label="关闭目录选择" onClick={onClose}><X size={17} /></button></header><p className="project-note">选择运行 Orbit 的机器上已有的目录。</p>
    <form onSubmit={event => { event.preventDefault(); open(path); }}><input aria-label="浏览目录路径" value={path} onChange={event => setPath(event.target.value)} placeholder="/absolute/path" /><button type="submit">打开</button></form>
    {error ? <p className="project-error" role="alert">{error}</p> : !result ? <p role="status">正在读取目录…</p> : <div className="project-directory-list">{result.parent && <button type="button" onClick={() => open(result.parent!)}><ArrowUp size={15} />上一级</button>}{result.directories.map(item => <button type="button" key={item.path} onClick={() => open(item.path)}><Folder size={15} />{item.name}</button>)}{!result.directories.length && <p className="project-note">此目录没有子目录。</p>}{result.truncated && <p className="project-note">只显示前 500 个目录，可输入路径继续浏览。</p>}</div>}
    <footer><button type="button" onClick={onClose}>取消</button><button className="orbit-primary" type="button" disabled={!result || Boolean(error)} onClick={() => onSelect(result!.directory)}>选择此目录</button></footer></dialog>;
}
