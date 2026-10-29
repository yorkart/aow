import { useEffect, useRef, useState } from 'react';
import { createProject, importSkills, listProjects, scanProject } from './api';
import type { Project, ScanResult } from './types';

export function SkillImport({ org, onCancel, onImported }: { org: string; onCancel: () => void; onImported: () => void }) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState('');
  const [adding, setAdding] = useState(false);
  const [projectName, setProjectName] = useState('');
  const [directory, setDirectory] = useState('');
  const [result, setResult] = useState<ScanResult>();
  const [selected, setSelected] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const lock = useRef(false);
  useEffect(() => {
    const abort = new AbortController();
    void listProjects(org, abort.signal).then(next => { setProjects(next); setProjectId(next[0]?.id ?? ''); }).catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '读取项目失败。'); });
    return () => abort.abort();
  }, [org]);
  const run = async (operation: () => Promise<void>) => {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError('');
    try { await operation(); }
    catch (error) { setError(error instanceof Error ? error.message : '操作失败。'); }
    finally { lock.current = false; setBusy(false); }
  };
  return <><header className="skills-heading"><div><h1>Import skills from project</h1><p>从本地项目扫描 SKILL.md，将选中的完整技能包复制到 Orbit。</p></div></header>
    <div className="skills-import-project"><label>当前组织的项目<select aria-label="选择项目" disabled={busy} value={projectId} onChange={event => { setProjectId(event.target.value); setResult(undefined); setSelected({}); }}><option value="">选择一个项目</option>{projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)}</select></label>
      <button type="button" disabled={busy || !projectId} onClick={() => { void run(async () => { setResult(undefined); setSelected({}); setResult(await scanProject(org, projectId)); }); }}>扫描技能</button><button type="button" disabled={busy} onClick={() => setAdding(!adding)}>登记本地项目</button></div>
    {projectId && <p className="skills-note"><code>{projects.find(project => project.id === projectId)?.directory}</code></p>}
    {(adding || !projects.length) && <form className="skills-project-form" onSubmit={event => { event.preventDefault(); void run(async () => {
      const project = await createProject(org, projectName, directory);
      setProjects(current => current.some(item => item.id === project.id) ? current : [...current, project]); setProjectId(project.id); setAdding(false); setResult(undefined); setSelected({});
    }); }}><p>为技能导入登记项目的本地工作目录。</p><label>项目名称<input required value={projectName} disabled={busy} onChange={event => setProjectName(event.target.value)} /></label><label>本地工作目录<input required placeholder="/absolute/path/to/project" value={directory} disabled={busy} onChange={event => setDirectory(event.target.value)} /></label><button type="submit" disabled={busy || !projectName.trim() || !directory.trim()}>保存项目</button></form>}
    {busy && <p role="status">正在处理…</p>}
    {error && <p className="skills-error" role="alert">{error}</p>}
    {result && <section className="skills-scan-results"><h2>扫描结果 <span>{result.skills.length}</span></h2>{result.warnings.length > 0 && <div className="skills-warnings">{result.warnings.map((warning, index) => <p key={index}>{warning}</p>)}</div>}
      {!result.skills.length && <p>此项目中没有找到可导入的技能。</p>}
      {result.skills.map(candidate => <div className="skills-import-row" key={candidate.path}><label className="skills-import-check"><input type="checkbox" aria-label={`导入 ${candidate.name}`} disabled={busy} checked={Object.hasOwn(selected, candidate.path)} onChange={event => setSelected(current => { if (event.target.checked) return { ...current, [candidate.path]: candidate.name }; const next = { ...current }; delete next[candidate.path]; return next; })} /><span><strong>{candidate.name}</strong><small>{candidate.description}</small><code>{candidate.path}/SKILL.md</code></span></label>
        {Object.hasOwn(selected, candidate.path) && <label>导入名称<input aria-label={`导入名称 ${candidate.name}`} maxLength={80} disabled={busy} value={selected[candidate.path]} onChange={event => setSelected(current => ({ ...current, [candidate.path]: event.target.value }))} /></label>}</div>)}
    </section>}
    <p className="skills-note">同名技能须改名后导入。导入保留包内目录和相对引用，不执行脚本，也不跟随源文件变化。</p>
    <footer className="skills-actions"><button type="button" disabled={busy} onClick={onCancel}>取消</button><button type="button" className="orbit-primary" disabled={busy || !result || !Object.keys(selected).length || Object.values(selected).some(name => !name.trim())} onClick={() => { void run(async () => { await importSkills(org, projectId, Object.entries(selected).map(([path, name]) => ({ path, name }))); onImported(); }); }}>导入所选技能</button></footer>
  </>;
}
