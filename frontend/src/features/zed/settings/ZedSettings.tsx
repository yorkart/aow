import { useEffect, useState } from 'react';
import { applyEdits, modify, parse, type ParseError } from 'jsonc-parser';
import { acpApi } from '../api';
import { failure, object, type AgentInfo, type SettingsFile } from '../types';
import { ConnectionProgress } from '../agent_ui/connection_progress';
import { useProgress } from '../agent_ui/use_progress';
import '../zed.css';

function agentSettings(content: string) {
  const errors: ParseError[] = []; const parsed: unknown = parse(content, errors, { allowTrailingComma: true });
  if (errors.length) throw new Error('请先修正 JSONC 配置中的语法错误。');
  return object(object(parsed).agent_servers);
}
export function ZedSettings({ active, onBusyChange, onDirtyChange }: { active: boolean; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void }) {
  const [saved, setSaved] = useState<SettingsFile>();
  const [content, setContent] = useState('');
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [installing, setInstalling] = useState('');
  const [failed, setFailed] = useState<string[]>([]);
  const progress = useProgress(installing);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [registry, setRegistry] = useState<AgentInfo[]>();
  const [search, setSearch] = useState('');
  const dirty = !!saved && content !== saved.content;
  useEffect(() => { onDirtyChange(dirty); }, [dirty, onDirtyChange]);
  useEffect(() => { if (active) onBusyChange(busy); }, [active, busy, onBusyChange]);
  useEffect(() => {
    if (!active || saved) return;
    let cancelled = false;
    void Promise.all([acpApi.settings(), acpApi.agents()]).then(([value, available]) => {
      if (!cancelled) { setSaved(value); setContent(value.content); setAgents(available); }
    }).catch(reason => { if (!cancelled) setError(failure(reason)); });
    return () => { cancelled = true; };
  }, [active, saved]);
  const run = async (operation: () => Promise<void>) => { setBusy(true); setError(''); setStatus(''); try { await operation(); } catch (reason) { setError(failure(reason)); } finally { setBusy(false); } };
  const save = async (draft: string) => {
    if (!saved) throw new Error('ACP 配置尚未加载。');
    const next = await acpApi.saveSettings(draft, saved.revision);
    setSaved(next); setContent(next.content); return next;
  };
  const install = (id: string) => run(async () => {
    const configured = agentSettings(content);
    const draft = id in configured ? content : applyEdits(content, modify(content, ['agent_servers', id], { type: 'registry', env: {}, default_config_options: {} }, { formattingOptions: { insertSpaces: true, tabSize: 2 } }));
    if (draft !== saved?.content) await save(draft);
    setInstalling(id);
    try {
      await acpApi.install(id);
      setFailed(values => values.filter(value => value !== id));
      setStatus(`${id} 已安装，可在 ACP 面板中新建会话。`);
    } catch (reason) { setFailed(values => [...new Set([...values, id])]); throw reason; }
    finally {
      setInstalling('');
      await acpApi.agents().then(setAgents).catch(reason => setError(failure(reason)));
    }
  });
  const addCustom = () => {
    try {
      if ('my-agent' in agentSettings(content)) throw new Error('已配置 my-agent。');
      setContent(applyEdits(content, modify(content, ['agent_servers', 'my-agent'], { type: 'custom', command: 'node', args: ['/path/to/agent.js'], env: {} }, { formattingOptions: { insertSpaces: true, tabSize: 2 } })));
      setStatus('已加入配置草稿，请保存。');
    } catch (reason) { setError(failure(reason)); }
  };
  const installButton = (agent: AgentInfo, registryEntry: boolean) => {
    const installed = agents.some(value => value.id === agent.id && value.installed);
    const configured = agents.some(value => value.id === agent.id);
    return <button type="button" disabled={busy || !agent.supported || installed} onClick={() => { void install(agent.id); }}>
      {installing === agent.id ? '安装中…' : installed ? '已安装' : failed.includes(agent.id) ? '重试安装' : configured || !registryEntry ? '安装' : '添加并安装'}
    </button>;
  };
  return <form className="project-aow-dialog-form zed-settings" onSubmit={event => { event.preventDefault(); void run(async () => { await save(content); setAgents(await acpApi.agents()); setStatus('已保存，新连接将使用更新后的配置。'); }); }}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>ACP</h2><p>安装 Registry Agent 或配置自定义启动命令。</p></div></div>
      {!!agents.length && <div className="zed-registry" aria-label="已配置的 ACP Agent">{agents.map(agent => <article key={agent.id}><div><strong>{agent.name}</strong><small>{agent.installed ? '可用于新建会话' : '尚未安装，请先完成安装'}</small></div>{installButton(agent, false)}</article>)}</div>}
      <div className="zed-actions"><button type="button" disabled={busy || !saved} onClick={() => { void run(async () => setRegistry(await acpApi.registry(true))); }}>浏览 ACP Registry</button><button type="button" disabled={busy || !saved} onClick={addCustom}>添加自定义 Agent</button></div>
      {registry && <div className="zed-registry" aria-label="ACP Registry"><input aria-label="搜索 ACP Registry" placeholder="搜索 Agent…" value={search} onChange={event => setSearch(event.target.value)} />{registry.filter(agent => `${agent.id} ${agent.name} ${agent.description}`.toLowerCase().includes(search.toLowerCase())).map(agent => <article key={agent.id}><div><strong>{agent.name}</strong><small>{agent.description}</small></div>{installButton(agent, true)}</article>)}</div>}
      {installing && <ConnectionProgress status={progress} message={`正在安装 ${installing}…`} />}
      <label className="project-aow-dialog-field"><span>{saved?.path ?? 'zed/settings.json'}</span><textarea className="zed-settings-editor" aria-label="ACP JSONC 配置" spellCheck={false} value={content} disabled={busy || !saved} onChange={event => setContent(event.target.value)} /></label>
      <small>支持注释和尾逗号。“添加并安装”会保存当前配置并立即安装 Agent，不会创建会话。手动加入 Registry 配置后，可在上方点击安装。</small>
      {error && <p className="zed-error" role="alert">{error}</p>}{status && <p role="status">{status}</p>}
    </div>
    <footer className="project-aow-dialog-footer"><button type="button" className="project-aow-dialog-button" disabled={busy} onClick={() => { if (!dirty || window.confirm('放弃未保存的 ACP 配置修改并重新加载？')) void run(async () => { const [next, available] = await Promise.all([acpApi.settings(), acpApi.agents()]); setSaved(next); setContent(next.content); setAgents(available); }); }}>重新加载</button><button type="submit" className="project-aow-dialog-button primary" disabled={busy || !saved || !dirty}>保存</button></footer>
  </form>;
}
