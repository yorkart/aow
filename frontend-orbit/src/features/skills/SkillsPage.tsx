import { useEffect, useState } from 'react';
import { ArrowLeft, Boxes, ChevronDown, ChevronRight, Plus, Search } from 'lucide-react';
import { listSkills } from './api';
import { SkillCreate } from './SkillCreate';
import { SkillEditor } from './SkillEditor';
import { SkillImport } from './SkillImport';
import type { Skill } from './types';
import './skills.css';

type Screen = { kind: 'list' | 'create' | 'import' } | { kind: 'edit'; id: string };
export function SkillsPage({ org, onDirtyChange, initialId }: { org: string; onDirtyChange: (dirty: boolean) => void; initialId?: string }) {
  const [screen, setScreen] = useState<Screen>(initialId ? { kind: 'edit', id: initialId } : { kind: 'list' });
  const back = () => setScreen({ kind: 'list' });
  if (screen.kind === 'edit') return <SkillEditor key={screen.id} org={org} id={screen.id} onBack={back} onDirtyChange={onDirtyChange} />;
  return <section className="skills-page">
    {screen.kind !== 'list' && <button className="skills-back" type="button" onClick={back}><ArrowLeft size={15} />返回 Installed skills</button>}
    {screen.kind === 'create' ? <SkillCreate org={org} onCancel={back} onCreated={id => setScreen({ kind: 'edit', id })} />
      : screen.kind === 'import' ? <SkillImport org={org} onCancel={back} onImported={back} />
      : <SkillList org={org} onOpen={id => setScreen({ kind: 'edit', id })} onNew={kind => setScreen({ kind })} />}
  </section>;
}

function SkillList({ org, onOpen, onNew }: { org: string; onOpen: (id: string) => void; onNew: (kind: 'create' | 'import') => void }) {
  const [skills, setSkills] = useState<Skill[]>();
  const [query, setQuery] = useState('');
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    const abort = new AbortController();
    void listSkills(org, abort.signal).then(setSkills).catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '无法读取技能库。'); });
    return () => abort.abort();
  }, [org, retry]);
  const filtered = skills?.filter(skill => `${skill.name} ${skill.description}`.toLowerCase().includes(query.toLowerCase()));
  return <>
    <header className="skills-heading"><div><h1>Installed skills</h1><p>管理当前组织的本地技能包。</p></div><details className="skills-new"><summary><Plus size={15} />New<ChevronDown size={13} /></summary><div className="skills-new-menu"><button type="button" onClick={() => onNew('create')}>Create a skill</button><button type="button" onClick={() => onNew('import')}>Import skills from project</button></div></details></header>
    <label className="skills-search"><Search size={15} /><input aria-label="搜索技能" placeholder="搜索名称或说明…" value={query} onChange={event => setQuery(event.target.value)} /></label>
    {error ? <div role="alert" className="skills-error">{error}<button type="button" onClick={() => { setError(''); setRetry(value => value + 1); }}>重试</button></div>
      : !skills ? <p role="status">正在读取技能库…</p>
      : !filtered?.length ? <div className="skills-empty"><Boxes size={26} /><h2>{query ? '没有匹配的技能' : '还没有安装技能'}</h2><p>{query ? '试试其他名称或说明。' : '通过 New 创建技能，或从本地项目导入完整技能包。'}</p></div>
      : <div className="skills-list">{filtered.map(skill => <button className="skills-list-row" type="button" key={skill.id} onClick={() => onOpen(skill.id)}>
        <Boxes size={20} /><span className="skills-list-info"><strong>{skill.name}</strong><span>{skill.description}</span></span><span className="skills-list-meta"><span>{skill.agent_count ? `${skill.agent_count} 位 Agent` : '未被引用'}</span><time dateTime={skill.updated_at}>{new Date(skill.updated_at).toLocaleString()}</time></span><ChevronRight size={16} />
      </button>)}</div>}
  </>;
}
