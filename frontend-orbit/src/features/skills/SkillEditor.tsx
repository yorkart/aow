import { useEffect, useRef, useState } from 'react';
import { ArrowLeft, Download, FilePlus2, FolderPlus, Pencil, Save, Trash2, Upload } from 'lucide-react';
import { deleteSkill, getSkill, saveSkill } from './api';
import { base64, dataUrl, download, mime, validPath, withParents } from './files';
import { FileTree } from './FileTree';
import { MarkdownPreview } from './MarkdownPreview';
import type { SkillDetail, SkillEntry } from './types';

export function SkillEditor({ org, id, onBack, onDirtyChange }: { org: string; id: string; onBack: () => void; onDirtyChange: (dirty: boolean) => void }) {
  const [skill, setSkill] = useState<SkillDetail>();
  const [files, setFiles] = useState<SkillEntry[]>([]);
  const [selected, setSelected] = useState('SKILL.md');
  const [preview, setPreview] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const [action, setAction] = useState<'file' | 'directory' | 'rename'>();
  const [path, setPath] = useState('');
  const upload = useRef<HTMLInputElement>(null);
  const lock = useRef(false);
  useEffect(() => {
    const abort = new AbortController();
    void getSkill(org, id, abort.signal).then(next => { if (!abort.signal.aborted) { setSkill(next); setFiles(next.files); } }).catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '无法读取技能。'); });
    return () => abort.abort();
  }, [org, id, retry]);
  useEffect(() => { onDirtyChange(dirty); return () => onDirtyChange(false); }, [dirty, onDirtyChange]);
  useEffect(() => {
    if (!dirty) return;
    const beforeUnload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', beforeUnload);
    return () => window.removeEventListener('beforeunload', beforeUnload);
  }, [dirty]);
  const run = async (operation: () => Promise<void>) => {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError('');
    try { await operation(); }
    catch (error) { setError(error instanceof Error ? error.message : '操作失败。'); }
    finally { lock.current = false; setBusy(false); }
  };
  const change = (next: SkillEntry[]) => { setFiles(withParents(next)); setDirty(true); setError(''); };
  const select = (path: string) => { if (!files.some(file => file.path === path)) { setError(`包内文件不存在：${path}`); return; } setSelected(path); setPreview(mime(path).startsWith('image/')); setAction(undefined); };
  const file = files.find(entry => entry.path === selected);
  const startAction = (next: 'file' | 'directory' | 'rename') => { setAction(next); setPath(next === 'rename' ? selected : file?.kind === 'directory' ? `${file.path}/` : ''); };
  const applyAction = () => {
    if (!validPath(path)) { setError('请填写有效的包内相对路径。'); return; }
    if (action === 'rename' && (selected === 'SKILL.md' || path === selected || path.startsWith(`${selected}/`))) { setError('此路径不能重命名。'); return; }
    const moved = action === 'rename' ? files.filter(item => item.path === selected || item.path.startsWith(`${selected}/`)) : [];
    const remaining = files.filter(item => !moved.includes(item));
    if (remaining.some(item => item.path === path || (item.path.startsWith(path + '/') && action !== 'rename'))) { setError('此路径已存在。'); return; }
    const next = action === 'rename' ? [...remaining, ...moved.map(item => ({ ...item, path: path + item.path.slice(selected.length) }))] : [...files, action === 'directory' ? { kind: 'directory' as const, path } : { kind: 'file' as const, path, encoding: 'utf8' as const, content: '', executable: false }];
    if (new Set(next.map(item => item.path)).size !== next.length) { setError('重命名与已有文件冲突。'); return; }
    try { change(next); setSelected(path); setAction(undefined); }
    catch (error) { setError(error instanceof Error ? error.message : '文件路径无效。'); }
  };
  const leave = () => { if (!busy && (!dirty || window.confirm('放弃尚未保存的文件修改？'))) onBack(); };
  if (!skill) return <section className="skills-page"><button className="skills-back" type="button" onClick={onBack}><ArrowLeft size={15} />返回 Installed skills</button>{error ? <p role="alert" className="skills-error">{error}<button type="button" onClick={() => { setError(''); setRetry(value => value + 1); }}>重试</button></p> : <p role="status">正在读取技能包…</p>}</section>;
  return <section className="skills-editor">
    <header className="skills-editor-heading"><button className="skills-back" type="button" disabled={busy} onClick={leave}><ArrowLeft size={15} />返回 Installed skills</button>
      <div className="skills-heading"><div><h1>{skill.name}</h1><p>{skill.description}</p></div><div className="skills-actions"><button className="orbit-primary" type="button" disabled={busy || !dirty} onClick={() => { void run(async () => { const next = await saveSkill(org, id, skill.current_version_id, files); setSkill(next); setFiles(next.files); setDirty(false); }); }}><Save size={14} />{busy ? '处理中…' : '保存'}</button><button type="button" title={skill.agent_count ? '请先解除 Agent 引用' : '删除技能'} disabled={busy || skill.agent_count > 0} onClick={() => { if (window.confirm(`删除技能「${skill.name}」？历史版本会保留。`)) void run(async () => { await deleteSkill(org, id); onBack(); }); }}><Trash2 size={14} />删除</button></div></div>
      <div className="skills-version"><span>版本 {skill.version}</span><span title={skill.content_hash}>{skill.content_hash.slice(0, 12)}</span><span>{skill.agent_count ? `${skill.agent_count} 位 Agent 引用` : '未被引用'}</span>{dirty && <strong role="status">有未保存的更改</strong>}</div>
      {skill.agents.length > 0 && <p className="skills-note">引用此技能的 Agent：{skill.agents.map(agent => agent.agent_id).join('、')}</p>}
      {skill.source_project_id && <p className="skills-note">来源项目路径：<code>{skill.source_path}</code> · 已导入独立副本</p>}
      {error && <p role="alert" className="skills-error">{error}</p>}
    </header>
    <div className="skills-editor-body"><aside className="skills-files"><div className="skills-file-actions"><button type="button" aria-label="新建文件" title="新建文件" disabled={busy} onClick={() => startAction('file')}><FilePlus2 size={15} /></button><button type="button" aria-label="新建目录" title="新建目录" disabled={busy} onClick={() => startAction('directory')}><FolderPlus size={15} /></button><button type="button" aria-label="上传文件" title="上传文件" disabled={busy} onClick={() => upload.current?.click()}><Upload size={15} /></button><button type="button" aria-label="重命名" title="重命名" disabled={busy || selected === 'SKILL.md'} onClick={() => startAction('rename')}><Pencil size={15} /></button><button type="button" aria-label="删除文件或目录" title="删除文件或目录" disabled={busy || selected === 'SKILL.md'} onClick={() => { if (window.confirm(`从草稿删除「${selected}」？点击保存后生效。`)) { change(files.filter(item => item.path !== selected && !item.path.startsWith(selected + '/'))); setSelected('SKILL.md'); setAction(undefined); } }}><Trash2 size={15} /></button></div>
      <input ref={upload} type="file" className="skills-upload" multiple aria-label="选择上传文件" onChange={event => { const incoming = Array.from(event.target.files ?? []); event.target.value = ''; void run(async () => {
        const prefix = file?.kind === 'directory' ? `${file.path}/` : '';
        const additions = [];
        for (const item of incoming) {
          const target = prefix + item.name;
          if (!validPath(target) || files.some(entry => entry.path === target)) throw new Error(`文件已存在或路径无效：${target}`);
          if (item.size > 2 * 1024 * 1024) throw new Error('单个文件不能超过 2 MiB。');
          const content = new Uint8Array(await item.arrayBuffer());
          let decoded: string | undefined;
          try { const value = new TextDecoder('utf-8', { fatal: true }).decode(content); if (!value.includes('\0')) decoded = value; } catch { /* Binary uploads keep their Base64 encoding. */ }
          additions.push({ path: target, kind: 'file' as const, encoding: decoded === undefined ? 'base64' as const : 'utf8' as const, content: decoded ?? base64(content), executable: false });
        }
        change([...files, ...additions]);
      }); }} />
      {action && <form className="skills-path-form" onSubmit={event => { event.preventDefault(); applyAction(); }}><label>{action === 'rename' ? '新路径' : action === 'directory' ? '目录路径' : '文件路径'}<input autoFocus required value={path} disabled={busy} onChange={event => setPath(event.target.value)} placeholder="references/checklist.md" /></label><div><button type="submit" disabled={busy}>确定</button><button type="button" onClick={() => setAction(undefined)}>取消</button></div></form>}
      <FileTree files={files} selected={selected} onSelect={select} />
    </aside><div className="skills-file-pane"><div className="skills-file-header"><code>{selected}</code><div>{file?.kind === 'file' && <><button type="button" aria-label="下载文件" title="下载文件" onClick={() => download(file)}><Download size={15} /></button>{file.encoding === 'utf8' && !mime(file.path).startsWith('image/') && <button type="button" aria-pressed={!preview} onClick={() => setPreview(false)}>编辑</button>}{(/\.md$/i.test(file.path) || mime(file.path).startsWith('image/')) && <button type="button" aria-pressed={preview} onClick={() => setPreview(true)}>预览</button>}</>}</div></div>
      {file?.kind === 'file' ? mime(file.path).startsWith('image/') ? <div className="skills-image"><img src={dataUrl(file)} alt={file.path} /></div> : file.encoding === 'base64' ? <div className="skills-empty"><p>二进制文件</p><button type="button" onClick={() => download(file)}>下载文件</button></div> : preview && /\.md$/i.test(file.path) ? <MarkdownPreview file={file} files={files} onOpen={select} /> : <textarea className="skills-code skills-text-editor" aria-label="文件内容" value={file.content} spellCheck={false} disabled={busy} onChange={event => change(files.map(item => item.path === file.path ? { ...file, content: event.target.value } : item))} /> : <div className="skills-empty"><p>选择文件以查看或编辑内容。</p></div>}
    </div></div>
  </section>;
}
