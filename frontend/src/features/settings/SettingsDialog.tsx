import { useEffect, useRef, useState } from 'react';
import { ArrowLeft, Bell, Bot, ChevronRight, Copy, FileText, GitPullRequest, MessageSquare, Network, NotebookPen, Pencil, RefreshCw, Settings, SquareTerminal, Trash2, X } from 'lucide-react';
import { aowApi } from '../../aow/aowApi';
import { parseNodeAddresses } from '../../aow/aowNodes';
import type { AowSettings } from '../../aow/types';
import type { AowAgent } from '../agents/types';
import { agentsApi } from '../agents/api';
import { AgentIcon } from '../agents/AgentIcon';
import { AgentArgumentsInput } from '../agents/AgentArgumentsInput';
import { argumentsDraft, normalizeArguments } from '../agents/arguments';
import { environmentDraft, parseEnvironment } from '../agents/environment';
import { agentTypes, builtinAgentType, aowAgentType, suggestedAgentExecutable } from '../agents/agentTypes';
import { defaultEditorSettings, useEditorSettings } from '../editor/editorSettings';
import { ReviewProviderSettings } from '../pr/ReviewProviderSettings';
import { ConfigurationSettings } from '../configuration/ConfigurationSettings';
import { NotificationSettingsPanel } from '../notifications/NotificationSettingsPanel';
import { LogoutButton } from '../auth/LogoutButton';
import { ServerEnvironmentFields, useServerEnvironment } from './ServerEnvironmentFields';

const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
const emptyAgentBaseline = JSON.stringify(['', '', '', '', '', '']);

const sections = [
  { id: 'configuration', label: 'Configuration', description: '选择配置仓库和版本', icon: Settings },
  { id: 'nodes', label: 'Nodes', description: '配置其他 AoW 节点', icon: Network },
  { id: 'editor', label: 'Editor', description: '配置文件编辑器', icon: FileText },
  { id: 'notes', label: 'Notes', description: '设置默认 Notes 根目录', icon: NotebookPen },
  { id: 'environment', label: 'Environment', description: '配置执行 PATH 和服务环境变量', icon: SquareTerminal },
  { id: 'agents', label: 'Agents', description: '配置 Agent 启动方式', icon: Bot },
  { id: 'im', label: 'IM', description: '配置飞书和微信机器人', icon: MessageSquare },
  { id: 'notifications', label: '通知', description: '选择任务完成通知方式', icon: Bell },
  { id: 'review', label: 'Pull Requests', description: 'Provider、CLI 和脚本', icon: GitPullRequest },
] as const;
type Section = typeof sections[number]['id'];

export function SettingsDialog({ agents, agentsError, mobile = false, onClose: closeDialog, onReload, onNodesChange }: {
  agents: AowAgent[]; agentsError?: string; mobile?: boolean; onClose: () => void;
  onReload: () => Promise<void>; onNodesChange: (addresses: string[]) => void;
}) {
  const [section, setSection] = useState<Section>('notes');
  const [overview, setOverview] = useState(mobile);
  const [reviewDirty, setReviewDirty] = useState(false);
  const [configurationDirty, setConfigurationDirty] = useState(false);
  const [notificationDirty, setNotificationDirty] = useState(false);
  const { updateEditorSettings } = useEditorSettings();
  const [editorWordWrap, setEditorWordWrap] = useState(false);
  const [editorSaved, setEditorSaved] = useState(false);
  const [settings, setSettings] = useState<AowSettings>();
  const [nodeAddresses, setNodeAddresses] = useState('');
  const [nodesSaved, setNodesSaved] = useState(false);
  const [notesBase, setNotesBase] = useState('');
  const [notesSaved, setNotesSaved] = useState(false);
  const [executionPath, setExecutionPath] = useState('');
  const [environmentSaved, setEnvironmentSaved] = useState(false);
  const [settingsLoading, setSettingsLoading] = useState(true);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const serverEnvironment = useServerEnvironment(section === 'environment' && !overview, setSettingsBusy);
  const [editingAgentId, setEditingAgentId] = useState<string>();
  const [agentId, setAgentId] = useState('');
  const [agentType, setAgentType] = useState<AowAgent['agent_type'] | ''>('');
  const [displayName, setDisplayName] = useState('');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState(() => argumentsDraft());
  const [env, setEnv] = useState(() => environmentDraft());
  const [agentSaved, setAgentSaved] = useState('');
  const [agentBaseline, setAgentBaseline] = useState(emptyAgentBaseline);
  const agentForm = useRef<HTMLFormElement>(null);
  const dialog = useRef<HTMLElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const dirty = reviewDirty || configurationDirty || serverEnvironment.dirty || mobile && (notificationDirty
    || !!settings && (notesBase !== settings.notes_base || nodeAddresses !== (settings.node_addresses ?? []).join('\n')
      || executionPath !== (settings.execution_path ?? []).join('\n') || editorWordWrap !== (settings.editor?.word_wrap ?? false))
    || JSON.stringify([agentId, agentType, displayName, command, args.text, env.text]) !== agentBaseline);
  const onClose = () => {
    if (busy || settingsBusy) return;
    if (!dirty || window.confirm('设置有未保存的修改，是否放弃并关闭？')) closeDialog();
  };
  const selectSection = (next: Section) => {
    // These panels own their drafts and are unmounted when switching categories.
    if (mobile && section !== next && notificationDirty && !window.confirm('当前设置有未保存的修改，是否放弃并切换？')) return;
    setSection(next); setOverview(false); setError('');
  };
  useEffect(() => { if (mobile) dialog.current?.focus(); }, [mobile, overview]);

  useEffect(() => {
    let active = true;
    void aowApi.settings()
      .then((next) => {
        if (!active) return;
        setSettings(next);
        setNotesBase(next.notes_base);
        setNodeAddresses((next.node_addresses ?? []).join('\n'));
        onNodesChange(next.node_addresses ?? []);
        setExecutionPath((next.execution_path ?? []).join('\n'));
        setEditorWordWrap(next.editor?.word_wrap ?? false);
        updateEditorSettings(next.editor ?? defaultEditorSettings);
      })
      .catch((reason) => { if (active) setError(message(reason)); })
      .finally(() => { if (active) setSettingsLoading(false); });
    return () => { active = false; };
  }, [updateEditorSettings, onNodesChange]);

  const saveNodes = async () => {
    setError('');
    setNodesSaved(false);
    try {
      const addresses = parseNodeAddresses(nodeAddresses);
      setSettingsBusy(true);
      const next = await aowApi.updateSettings({ nodeAddresses: addresses });
      setSettings(next);
      setNodeAddresses(next.node_addresses.join('\n'));
      onNodesChange(next.node_addresses);
      setNodesSaved(true);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const saveEditor = async () => {
    setSettingsBusy(true);
    setError('');
    setEditorSaved(false);
    try {
      const next = await aowApi.updateSettings({ editor: { word_wrap: editorWordWrap } });
      setSettings(next);
      setEditorWordWrap(next.editor.word_wrap);
      updateEditorSettings(next.editor);
      setEditorSaved(true);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const saveNotesRoot = async () => {
    setNotesSaved(false);
    const target = notesBase.trim();
    if (!target.startsWith('/')) {
      setError('Notes 根目录必须是服务端上的绝对路径。');
      return;
    }
    setSettingsBusy(true);
    setError('');
    try {
      const next = await aowApi.updateSettings({ notesBase: target });
      setSettings(next);
      setNotesBase(next.notes_base);
      setNotesSaved(true);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const readEnvironment = async () => {
    setSettingsBusy(true);
    setError('');
    setEnvironmentSaved(false);
    try {
      setExecutionPath((await aowApi.discoveredPath()).join('\n'));
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const saveEnvironment = async () => {
    const paths = executionPath.split('\n').map((path) => path.trim()).filter(Boolean);
    if (!paths.length || paths.some((path) => !path.startsWith('/') || path.includes(':'))) {
      setError('PATH 每行填写一个服务端绝对目录路径，不使用冒号分隔。');
      return;
    }
    setSettingsBusy(true);
    setError('');
    setEnvironmentSaved(false);
    try {
      const next = await aowApi.updateSettings({ executionPath: paths });
      setSettings(next);
      setExecutionPath(next.execution_path.join('\n'));
      setEnvironmentSaved(true);
      await onReload();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const resetAgentForm = () => {
    setEditingAgentId(undefined);
    setAgentId('');
    setAgentType('');
    setDisplayName('');
    setCommand('');
    setArgs(argumentsDraft());
    setEnv(environmentDraft());
    setAgentBaseline(emptyAgentBaseline);
    setAgentSaved('');
    setError('');
  };

  const loadAgent = (agent: AowAgent, copy = false) => {
    const name = copy ? `${agent.display_name}（副本）` : agent.display_name;
    const nextArgs = argumentsDraft(agent.args);
    const nextEnv = environmentDraft(agent.env);
    setEditingAgentId(copy ? undefined : agent.id);
    setAgentId(copy ? '' : agent.id);
    setAgentType(aowAgentType(agent) ?? '');
    setDisplayName(name);
    setCommand(agent.command ?? agent.executable ?? '');
    setArgs(nextArgs);
    setEnv(nextEnv);
    setAgentBaseline(copy ? emptyAgentBaseline : JSON.stringify([agent.id, aowAgentType(agent) ?? '', name, agent.command ?? agent.executable ?? '', nextArgs.text, nextEnv.text]));
    setAgentSaved('');
    setError('');
    agentForm.current?.querySelector('select')?.scrollIntoView({ block: 'nearest' });
    if (copy) agentForm.current?.querySelector('input')?.focus({ preventScroll: true });
  };

  const selectAgentType = (value: string) => {
    const nextType = builtinAgentType(value) ?? '';
    if (!command.trim() || command === suggestedAgentExecutable(agents, agentType)) {
      setCommand(suggestedAgentExecutable(agents, nextType));
    }
    setAgentType(nextType);
    setAgentSaved('');
    setError('');
  };

  const removeAgent = async (id: string) => {
    setBusy(true);
    setError('');
    setAgentSaved('');
    try {
      await agentsApi.removeAgent(id);
      if (editingAgentId === id) resetAgentForm();
      await onReload();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!/^[A-Za-z0-9_-]{1,80}$/.test(agentId)) {
      setError('Agent ID 必填，限 1–80 个字母、数字、- 或 _。');
      return;
    }
    if (agents.some(agent => agent.id === agentId && agent.id !== editingAgentId)) {
      setError('Agent ID 已存在，请使用其他 ID。');
      return;
    }
    if (!agentType) {
      setError('请选择 Agent 类型。');
      return;
    }
    setBusy(true);
    setError('');
    setAgentSaved('');
    try {
      const parsed = normalizeArguments(args, command).values;
      const environment = parseEnvironment(env);
      await agentsApi.registerAgent({
        id: agentId,
        agentType,
        displayName, command, args: parsed,
        env: environment,
      }, editingAgentId);
      resetAgentForm();
      setAgentSaved(`${displayName} 配置已保存。`);
      await onReload();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  };

  return <div className={mobile ? 'mobile-settings-backdrop' : 'project-aow-modal-backdrop'} onPointerDown={onClose}>
    <section ref={dialog} tabIndex={-1} className={`project-aow-modal project-aow-dialog project-aow-settings${mobile ? ' mobile-settings' : ''}`} role="dialog" aria-modal="true" aria-labelledby="aow-settings-title" onPointerDown={(event) => event.stopPropagation()}
      onKeyDown={event => {
        if (event.key === 'Escape') { event.stopPropagation(); if (mobile && !overview && !busy && !settingsBusy) setOverview(true); else onClose(); }
        if (event.key === 'Tab') {
          const controls = [...event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], summary, [tabindex="0"]')].filter(element => element.getClientRects().length > 0);
          const first = controls[0]; const last = controls.at(-1);
          if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog.current)) { event.preventDefault(); last?.focus(); }
          else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
        }
      }}>
      {mobile ? <header className="mobile-settings-header">
        <button type="button" className="mobile-icon-button" aria-label={overview ? '返回主页' : '返回设置分类'} disabled={busy || settingsBusy} onClick={() => { if (overview) onClose(); else setOverview(true); }}><ArrowLeft /></button>
        <strong id="aow-settings-title">{overview ? '设置' : sections.find(item => item.id === section)?.label}</strong>
        {!overview && <button type="button" className="mobile-icon-button" aria-label="关闭设置" disabled={busy || settingsBusy} onClick={onClose}><X /></button>}
      </header> : <header><div><Settings /><strong id="aow-settings-title">设置</strong></div><button type="button" title="关闭" aria-label="关闭" disabled={busy || settingsBusy} onClick={onClose}><X /></button></header>}
      <div className="project-aow-settings-body">
        <nav className="project-aow-settings-nav" aria-label="设置分类" hidden={mobile && !overview}>
          {sections.map(({ id, label, description, icon: Icon }) => <button key={id} type="button" disabled={busy || settingsBusy} aria-current={!overview && section === id ? 'page' : undefined} className={!overview && section === id ? 'active' : ''} onClick={() => selectSection(id)}>
            <Icon /><span><strong>{label}</strong><small>{description}</small></span>
            {id === 'agents' && <i>{agents.filter(agent => agent.available).length}</i>}
            {mobile && <ChevronRight />}
          </button>)}
          {!mobile && <LogoutButton disabled={busy || settingsBusy} />}
        </nav>
        <div className="project-aow-settings-content" hidden={mobile && overview}>
          <div className="review-provider-settings-host" hidden={section !== 'review'}><ReviewProviderSettings active={section === 'review'} onBusyChange={setSettingsBusy} onDirtyChange={setReviewDirty} /></div>
          <div className="configuration-settings-host" hidden={section !== 'configuration'}><ConfigurationSettings active={section === 'configuration'} onBusyChange={setSettingsBusy} onDirtyChange={setConfigurationDirty} /></div>
          {section === 'review' || section === 'configuration' ? null : section === 'nodes' ? (
            <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); void saveNodes(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Nodes</h2><p>配置其他机器上部署的 AoW，点击左上角 Logo 或 AoW 文字即可切换。</p></div></div>
                <label className="project-aow-dialog-field"><span>节点地址（每行一个）</span><textarea aria-label="节点地址" spellCheck={false} rows={10} value={nodeAddresses} disabled={settingsLoading || settingsBusy || !settings} onChange={event => { setNodeAddresses(event.target.value); setNodesSaved(false); setError(''); }} placeholder={'https://aow-a.example.com\nhttp://192.168.1.20:8080'} /></label>
                <p className="project-aow-form-intro">填写完整的 http:// 或 https:// 地址，可以包含当前节点。同一份列表可复制到所有节点；下拉菜单会按协议、域名/IP 和端口自动过滤当前节点。留空并保存可清空列表。</p>
                {nodesSaved ? <p role="status">节点地址已保存。</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !settings}>{settingsBusy ? '保存中…' : '保存'}</button></footer>
            </form>
          ) : section === 'im' || section === 'notifications' ? <NotificationSettingsPanel section={section} onBusyChange={setSettingsBusy} onDirtyChange={setNotificationDirty} /> : section === 'editor' ? (
            <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); void saveEditor(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Editor</h2><p>设置文件编辑器的全局默认行为。</p></div></div>
                <label className="project-aow-dialog-checkbox"><input type="checkbox" checked={editorWordWrap} disabled={settingsLoading || settingsBusy || !settings} onChange={event => { setEditorWordWrap(event.target.checked); setEditorSaved(false); setError(''); }} /><span>Word Wrap（自动换行）</span></label>
                <p className="project-aow-form-intro">开启后，长行会根据编辑区宽度自动折行。每个文件 Tab 可通过右上角开关临时切换，刷新或重新打开后恢复使用全局配置。</p>
                {editorSaved ? <p role="status">已保存 Editor 配置。</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !settings}>{settingsBusy ? '保存中…' : '保存'}</button></footer>
            </form>
          ) : section === 'notes' ? <>
            <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void saveNotesRoot(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Notes</h2><p>新注册项目在未指定 Notes path 时，会以仓库地址映射到这个根目录下。</p></div></div>
                <label className="project-aow-dialog-field"><span>Notes root</span><input className="project-aow-dialog-monospace" autoFocus={!mobile} spellCheck={false} value={notesBase} disabled={settingsLoading || settingsBusy} onChange={(event) => { setNotesBase(event.target.value); setNotesSaved(false); setError(''); }} placeholder="/absolute/path/to/aow" required /></label>
                <small className="project-aow-dialog-path-hint">当前根目录：<code title={settings?.notes_base}>{settings?.notes_base ?? '加载中…'}</code></small>
                <p className="project-aow-form-intro">仅修改默认根目录，不迁移笔记。已有项目保持原 Notes 路径；如需切换，请在项目菜单中使用「绑定 Notes 目录」。</p>
                {notesSaved ? <p role="status">Notes root 已保存。已有项目仍使用原 Notes 路径，如需切换，请在项目菜单中使用「绑定 Notes 目录」。</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !notesBase.trim()}>{settingsBusy ? '保存中…' : '保存'}</button></footer>
            </form>
          </> : section === 'environment' ? <>
            <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void saveEnvironment(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Environment</h2><p>配置执行 PATH，编辑 AoW 服务的环境变量文件。</p></div></div>
                <label className="project-aow-dialog-field"><span>PATH 目录（从上到下优先）</span><textarea aria-label="PATH 目录" spellCheck={false} rows={10} value={executionPath} disabled={settingsLoading || settingsBusy} onChange={(event) => { setExecutionPath(event.target.value); setEnvironmentSaved(false); setError(''); }} placeholder={'/opt/python/3.11/bin\n/usr/local/bin\n/usr/bin\n/bin'} required /></label>
                <p className="project-aow-form-intro">每行一个服务器上的绝对目录。要优先使用某个 Python，请把包含 python3 的目录放在前面；这里不填写可执行文件，也不展开 ~、$HOME 或 $PATH。</p>
                <p className="project-aow-form-intro">保存后对后续执行生效，已有任务无需重新保存。正在运行的任务保持原环境。</p>
                {environmentSaved ? <p role="status">执行环境已保存。</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
                <ServerEnvironmentFields environment={serverEnvironment} busy={settingsBusy} />
              </div>
              <footer className="project-aow-dialog-footer"><button type="button" className="project-aow-dialog-button" disabled={settingsLoading || settingsBusy} onClick={() => void readEnvironment()}><RefreshCw size={14} />从本机环境读取</button><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !executionPath.trim()}>{settingsBusy ? '处理中…' : '保存 PATH'}</button></footer>
            </form>
          </> : <>
            <form className="project-aow-dialog-form" ref={agentForm} onSubmit={(event) => { event.preventDefault(); void save(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Agents</h2><p>目前支持 Claude Code、Codex、TraeCode CLI、Hermes 和 Pi。每种类型可注册多个配置。</p></div><button type="button" className="project-aow-dialog-button" title="重新探测本地 Agent" disabled={busy} onClick={() => void onReload().catch((reason) => setError(message(reason)))}><RefreshCw />刷新</button></div>
                {agentsError && <div className="project-aow-error" role="alert">{agentsError}</div>}
                <div className="project-aow-agent-list">
                  {agents.map((agent) => <div className="project-aow-agent-row" key={agent.id}>
                    <span className={`project-aow-agent-dot ${agent.available ? 'available' : ''}`} />
                    <AgentIcon agentId={aowAgentType(agent)} />
                    <div><strong>{agent.display_name}</strong><code>{agent.id}</code><small>{agentTypes.find(type => type.id === aowAgentType(agent))?.label ?? '未设置类型，请编辑补选'}</small><code>{agent.executable ?? '未找到 executable'}</code></div>
                    <small>{agent.source === 'detected' ? 'Auto detected' : 'Configured'}</small>
                    <button type="button" className="project-aow-agent-edit" title={`编辑 ${agent.display_name}`} aria-pressed={editingAgentId === agent.id} disabled={busy} onClick={() => loadAgent(agent)}><Pencil /></button>
                    <button type="button" className="project-aow-agent-copy" title={`复制 ${agent.display_name}`} aria-label={`复制 ${agent.display_name}`} disabled={busy} onClick={() => loadAgent(agent, true)}><Copy /></button>
                    {agent.source === 'configured' ? <button type="button" title="移除配置" disabled={busy} onClick={() => void removeAgent(agent.id)}><Trash2 /></button> : null}
                  </div>)}
                  {!agents.length ? <p className="project-aow-empty">尚未发现本地 Agent，可在下方注册。</p> : null}
                </div>
                <h3>{editingAgentId ? '编辑 Agent 配置' : '注册 Agent 配置'}</h3>
                <label className="project-aow-dialog-field"><span>Agent ID <em>必填</em></span><input aria-label="Agent ID" value={agentId} readOnly={!!editingAgentId} disabled={busy} onChange={event => { setAgentId(event.target.value); setAgentSaved(''); setError(''); }} placeholder="例如：work-codex" pattern={'[A-Za-z0-9_\\-]+'} maxLength={80} autoCapitalize="none" spellCheck={false} required /><small>唯一标识，限字母、数字、- 和 _，创建后不可修改；如需调整，请删除后重新创建。CLI 使用 --agent-id 指定此配置。</small></label>
                <label className="project-aow-dialog-field"><span>Agent 类型 <em>必填</em></span><div className="project-aow-agent-type"><AgentIcon agentId={agentType} /><select aria-label="Agent 类型" value={agentType ?? ''} disabled={busy || !!builtinAgentType(editingAgentId)} onChange={(event) => selectAgentType(event.target.value)} required><option value="" disabled>请选择 Agent 类型</option>{agentTypes.map(type => <option key={type.id} value={type.id}>{type.label}</option>)}</select></div></label>
                <p className="project-aow-form-intro">选择类型后，优先填入自动探测的可执行文件路径，否则使用第一条同类配置的路径。名称、启动命令、参数和环境变量均可编辑。</p>
                <label className="project-aow-dialog-field"><span>Display name</span><input value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} placeholder="例如：工作用 Codex" required /></label>
                <label className="project-aow-dialog-field"><span>Executable</span><input className="project-aow-dialog-monospace" spellCheck={false} value={command} disabled={busy} onChange={(event) => setCommand(event.target.value)} placeholder="命令名或 /absolute/path" required /></label>
                <AgentArgumentsInput value={args} onChange={setArgs} executable={command} disabled={busy} onError={setError} />
                <label className="project-aow-dialog-field"><span>Environment variables</span><textarea aria-label="Environment variables" spellCheck={false} rows={4} value={env.text} disabled={busy} onChange={(event) => setEnv({ ...env, text: event.target.value })} placeholder={'BASE_URL=https://example.com\nAPI_KEY=your-key'} /></label>
                <p className="project-aow-form-intro">自动继承启动环境。每行填写一个 key=value，只填写需要新增或覆盖的变量；留空即可保留继承的环境。值按原文保存，无需加引号。</p>
                <p className="project-aow-form-intro">保存后用于新启动的终端 Agent。PATH 统一在 Environment 中配置。移除内置 Agent 的配置后会恢复自动探测。</p>
                {agentSaved ? <p role="status">{agentSaved}</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer">{editingAgentId ? <button type="button" className="project-aow-dialog-button" disabled={busy} onClick={resetAgentForm}>取消编辑</button> : null}<button type="submit" className="project-aow-dialog-button primary" disabled={busy || !agentType}>{busy ? '保存中…' : editingAgentId ? '保存配置' : '注册'}</button></footer>
            </form>
          </>}
        </div>
      </div>
    </section>
  </div>;
}
