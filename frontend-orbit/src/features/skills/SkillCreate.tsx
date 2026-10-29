import { useRef, useState, type FormEvent } from 'react';
import { createSkill } from './api';

export function SkillCreate({ org, onCancel, onCreated }: { org: string; onCancel: () => void; onCreated: (id: string) => void }) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [instructions, setInstructions] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const submitting = useRef(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (submitting.current) return;
    submitting.current = true;
    setBusy(true);
    setError('');
    try { onCreated((await createSkill(org, { name, description, instructions })).id); }
    catch (error) { setError(error instanceof Error ? error.message : '创建失败。'); }
    finally { submitting.current = false; setBusy(false); }
  };
  return <><header className="skills-heading"><div><h1>Create a skill</h1><p>名称、说明和指令将生成技能入口 SKILL.md。</p></div></header>
    <form className="skills-form" onSubmit={event => { void submit(event); }}>
      <label>名称<input autoFocus required maxLength={80} value={name} disabled={busy} placeholder="code-review" onChange={event => setName(event.target.value)} /></label>
      <label>说明<textarea required maxLength={2000} rows={3} value={description} disabled={busy} placeholder="描述技能的用途与适用场景" onChange={event => setDescription(event.target.value)} /></label>
      <label>指令正文<textarea required rows={16} className="skills-code" value={instructions} disabled={busy} placeholder="编写 Markdown 指令…" onChange={event => setInstructions(event.target.value)} /></label>
      {error && <p role="alert" className="skills-error">{error}</p>}
      <footer className="skills-actions"><button type="button" disabled={busy} onClick={onCancel}>取消</button><button className="orbit-primary" type="submit" disabled={busy || !name.trim() || !description.trim() || !instructions.trim()}>{busy ? '正在创建…' : '创建技能'}</button></footer>
    </form></>;
}
