import { useState } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { saveVariables } from './api';
import { useDirty, useOperation } from './hooks';
import type { AgentVariable, PanelProps } from './types';

export function AgentVariables({ org, agent, onSaved, onDirtyChange }: PanelProps) {
  const initial = agent.variables.map(variable => ({ ...variable, value: variable.value ?? '' }));
  const [rows, setRows] = useState(initial); const dirty = JSON.stringify(rows) !== JSON.stringify(initial); useDirty(dirty, onDirtyChange);
  const { busy, error, perform } = useOperation();
  const change = (index: number, patch: Partial<AgentVariable>) => setRows(rows.map((row, i) => i === index ? { ...row, ...patch } : row));
  return <><header className="agent-panel-heading"><div><h2>Secrets & variables</h2><p className="agent-note">仅注入此 Agent 的执行进程。AI 认证和 Orbit 管理的运行环境字段不能覆盖。</p></div><button type="button" disabled={busy || rows.length >= 100} onClick={() => setRows([...rows, { name: '', kind: 'variable', value: '' }])}><Plus size={15} />新增变量</button></header>
    <form onSubmit={event => { event.preventDefault(); void perform(async () => {
      const values = rows.map(row => { const value: AgentVariable = { id: row.id, name: row.name, kind: row.kind }; if (row.kind === 'variable' || row.value !== '' || !row.configured) value.value = row.value; return value; });
      onSaved(await saveVariables(org, agent, values));
    }); }}><div className="agent-variables-scroll"><table className="agent-variables"><thead><tr><th scope="col">名称</th><th scope="col">类型</th><th scope="col">值</th><th scope="col">操作</th></tr></thead><tbody>{rows.map((row, i) => <tr key={row.id ?? `new-${i}`}><td><input aria-label={`变量名称 ${i + 1}`} required maxLength={80} pattern="[A-Za-z_][A-Za-z0-9_]*" disabled={busy} value={row.name} onChange={event => change(i, { name: event.target.value })} /></td><td><select aria-label={`变量类型 ${i + 1}`} disabled={busy} value={row.kind} onChange={event => change(i, { kind: event.target.value as AgentVariable['kind'], value: '', configured: false })}><option value="variable">Variable</option><option value="secret">Secret</option></select></td><td><input aria-label={`变量值 ${i + 1}`} type={row.kind === 'secret' ? 'password' : 'text'} autoComplete="off" required={row.kind === 'secret' && !row.configured} disabled={busy} value={row.value} placeholder={row.kind === 'secret' && row.configured ? '已配置，留空保留；输入新值替换' : '输入值'} onChange={event => change(i, { value: event.target.value })} /></td><td><button type="button" aria-label={`删除变量 ${i + 1}`} disabled={busy} onClick={() => setRows(rows.filter((_, index) => index !== i))}><Trash2 size={14} /></button></td></tr>)}</tbody></table></div>
      {!rows.length && <p className="agent-note">尚未配置变量。</p>}{error && <p className="agent-error" role="alert">{error}</p>}<footer className="agent-actions"><button className="orbit-primary" type="submit" aria-busy={busy} disabled={busy || !dirty}>保存变量</button></footer></form></>;
}
