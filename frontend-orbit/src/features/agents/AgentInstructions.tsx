import { useEffect, useState } from 'react';
import { FilePlus, Pencil, Trash2 } from 'lucide-react';
import { MarkdownPreview } from '../skills/MarkdownPreview';
import type { SkillEntry, SkillFile } from '../skills/types';
import { getInstructions, saveInstructions } from './api';
import { useDirty, useOperation } from './hooks';
import type { PanelProps } from './types';
import '../skills/skills.css';

export function AgentInstructions({ org, agent, onSaved, onDirtyChange }: PanelProps) {
  const [files, setFiles] = useState<SkillEntry[]>(); const [baseline, setBaseline] = useState(''); const [version, setVersion] = useState('');
  const [selected, setSelected] = useState('AGENTS.md'); const [preview, setPreview] = useState(false); const [loadError, setLoadError] = useState(''); const [retry, setRetry] = useState(0); const [fileError, setFileError] = useState('');
  const dirty = files !== undefined && JSON.stringify(files) !== baseline; useDirty(dirty, onDirtyChange);
  const { busy, error, perform } = useOperation();
  useEffect(() => {
    const abort = new AbortController();
    void getInstructions(org, agent, abort.signal).then(value => { if (!abort.signal.aborted) { setFiles(value.files); setBaseline(JSON.stringify(value.files)); setVersion(value.version_id); } }).catch((error: unknown) => { if (!abort.signal.aborted) setLoadError(error instanceof Error ? error.message : '无法读取指令。'); });
    return () => abort.abort();
  }, [org, agent, retry]);
  const validPath = (path: string) => path.length <= 240 && /\.md$/i.test(path) && !/[\\\p{Cc}]/u.test(path) && !path.split('/').some(part => !part.trim() || part === '.' || part === '..');
  const changePath = (rename: boolean) => {
    if (!files) return;
    const path = window.prompt(rename ? '新的 Markdown 相对路径' : '新建 Markdown 文件的相对路径', rename ? selected : 'checklist.md')?.trim();
    if (!path || rename && path === selected) return;
    if (!validPath(path) || files.some(file => file.path === path) || files.some(file => file.kind === 'file' && path.startsWith(file.path + '/'))) { setFileError('请输入未占用的 Markdown 相对路径，不能包含 ..。'); return; }
    setFileError('');
    const next: SkillEntry[] = rename ? files.map(file => file.path === selected ? { ...file, path } : file) : [...files, { path, kind: 'file', encoding: 'utf8', content: '', executable: false }];
    // Include parent directories so nested files remain visible and relative links work.
    const parents = path.split('/'); parents.pop(); while (parents.length) { const parent = parents.join('/'); if (!next.some(file => file.path === parent)) next.push({ path: parent, kind: 'directory' }); parents.pop(); }
    setFiles(next); setSelected(path); setPreview(false);
  };
  if (loadError) return <p className="agent-error" role="alert">{loadError}<button type="button" onClick={() => { setLoadError(''); setRetry(value => value + 1); }}>重试</button></p>;
  if (!files) return <p role="status">正在读取指令…</p>;
  const file = files.find(item => item.path === selected && item.kind === 'file') as SkillFile | undefined;
  return <><header className="agent-panel-heading"><div><h2>Instructions</h2><p className="agent-note">AGENTS.md 是固定入口。辅助文件可使用包内相对路径引用。</p></div><button className="orbit-primary" type="button" aria-busy={busy} disabled={busy || !dirty} onClick={() => { void perform(async () => onSaved(await saveInstructions(org, agent, version, files))); }}>保存指令</button></header>
    <div className="agent-instructions"><aside aria-label="指令文件"><div className="agent-file-actions"><button type="button" disabled={busy} onClick={() => changePath(false)}><FilePlus size={14} />新建文件</button></div>{files.filter(file => file.kind === 'file').sort((a, b) => a.path === 'AGENTS.md' ? -1 : b.path === 'AGENTS.md' ? 1 : a.path.localeCompare(b.path)).map(file => <button className="agent-file-row" type="button" key={file.path} aria-pressed={selected === file.path} onClick={() => { setSelected(file.path); setPreview(false); }}>{file.path}</button>)}</aside>
      <div className="agent-instruction-content">{file && <><div className="agent-file-toolbar"><strong>{file.path}</strong><button type="button" aria-pressed={!preview} onClick={() => setPreview(false)}>编辑</button><button type="button" aria-pressed={preview} onClick={() => setPreview(true)}>预览</button><button type="button" aria-label="重命名指令文件" disabled={busy || selected === 'AGENTS.md'} onClick={() => changePath(true)}><Pencil size={14} /></button><button type="button" aria-label="删除指令文件" disabled={busy || selected === 'AGENTS.md'} onClick={() => { if (window.confirm(`删除 ${selected}？`)) { setFiles(files.filter(item => item.path !== selected)); setSelected('AGENTS.md'); } }}><Trash2 size={14} /></button></div>
        {preview ? <MarkdownPreview file={file} files={files} onOpen={path => { if (files.some(item => item.kind === 'file' && item.path === path)) setSelected(path); else setFileError('引用的指令文件不存在。'); }} /> : <textarea className="agent-instruction-editor" aria-label="指令内容" spellCheck={false} disabled={busy} value={file.content} onChange={event => setFiles(files.map(item => item.path === selected ? { ...file, content: event.target.value } : item))} />}</>}</div></div>
    {(error || fileError) && <p className="agent-error" role="alert">{error || fileError}</p>}</>;
}
