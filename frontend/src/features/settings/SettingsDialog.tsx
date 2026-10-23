import { t } from '../../lib/i18n';
import type { MutableRefObject } from 'react';
import { useEffect, useRef, useState } from 'react';
import { ArrowLeft, Bell, Bot, ChevronRight, FileText, GitBranch, MessageSquare, Network, NotebookPen, Palette, RefreshCw, Settings, SquareTerminal, X } from 'lucide-react';
import { aowApi } from '../../aow/aowApi';
import { parseNodeAddresses } from '../../aow/aowNodes';
import type { AowSettings } from '../../aow/types';
import type { AowAgent } from '../agents/types';
import { agentsApi } from '../agents/api';
import { AgentIcon } from '../agents/AgentIcon';
import { AcpIcon } from '../agents/AcpIcon';
import { AgentArgumentsInput } from '../agents/AgentArgumentsInput';
import { argumentsDraft, normalizeArguments } from '../agents/arguments';
import { environmentDraft, parseEnvironment } from '../agents/environment';
import { agentTypes, builtinAgentType, aowAgentType, suggestedAgentExecutable } from '../agents/agentTypes';
import { defaultEditorSettings, useEditorSettings } from '../editor/editorSettings';
import { GitPlatformProviderSettings } from '../git-platform-providers/GitPlatformProviderSettings';
import { AcpSettings } from '../acp';
import { ConfigurationSettings } from '../configuration/ConfigurationSettings';
import { NotificationSettingsPanel } from '../notifications/NotificationSettingsPanel';
import { LogoutButton } from '../auth/LogoutButton';
import { ServerEnvironmentFields, useServerEnvironment } from './ServerEnvironmentFields';
import { SettingsActionDialog } from './SettingsActionDialog';
import { AgentList } from './AgentList';
import { AppearanceSettings } from './AppearanceSettings';

const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
const emptyAgentBaseline = JSON.stringify(['', '', '', '', '', '']);

const sections = [
  { id: 'appearance', label: t("navigation.appearance"), description: t("settings.theme_and_interface_colors"), icon: Palette },
  { id: 'configuration', label: t("navigation.configuration"), description: t("settings.choose_configuration_repository_and_version"), icon: Settings },
  { id: 'nodes', label: t("navigation.nodes"), description: t("settings.configure_other_aow_nodes"), icon: Network },
  { id: 'editor', label: t("navigation.editor"), description: t("settings.configure_the_file_editor"), icon: FileText },
  { id: 'notes', label: t("navigation.notes"), description: t("settings.set_the_default_notes_root_directory"), icon: NotebookPen },
  { id: 'environment', label: t("navigation.environment"), description: t("settings.configure_execution_path_and_service_environment_variables"), icon: SquareTerminal },
  { id: 'acp', label: 'ACP', description: t("settings.configure_acp_agents"), icon: AcpIcon },
  { id: 'agents', label: t("navigation.agents"), description: t("settings.configure_agent_launch_methods"), icon: Bot },
  { id: 'im', label: 'IM', description: t("settings.configure_feishu_and_wechat_bots"), icon: MessageSquare },
  { id: 'notifications', label: t("notifications.notifications_2"), description: t("settings.choose_task_completion_notifications"), icon: Bell },
  { id: 'git-platforms', label: t("git_platform.title"), description: t("settings.providers_cli_and_scripts"), icon: GitBranch },
] as const;
type Section = typeof sections[number]['id'];

export function SettingsDialog({ agents, agentsError, mobile = false, embedded = false, closeRequest, onClose: closeDialog, onReload, onNodesChange }: {
  embedded?: boolean; closeRequest?: MutableRefObject<() => void>;
  agents: AowAgent[]; agentsError?: string; mobile?: boolean; onClose: () => void;
  onReload: () => Promise<void>; onNodesChange: (addresses: string[]) => void;
}) {
  const [section, setSection] = useState<Section>(sections[0].id);
  const [overview, setOverview] = useState(mobile);
  const [acpDirty, setAcpDirty] = useState(false);
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
  const [agentDialog, setAgentDialog] = useState<'new' | 'copy' | 'edit'>();
  const [editingAgentId, setEditingAgentId] = useState<string>();
  const [agentId, setAgentId] = useState('');
  const [agentType, setAgentType] = useState<AowAgent['agent_type'] | ''>('');
  const [displayName, setDisplayName] = useState('');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState(() => argumentsDraft());
  const [env, setEnv] = useState(() => environmentDraft());
  const [agentSaved, setAgentSaved] = useState('');
  const [agentRefreshError, setAgentRefreshError] = useState('');
  const [agentBaseline, setAgentBaseline] = useState(emptyAgentBaseline);
  const agentForm = useRef<HTMLFormElement>(null);
  const dialog = useRef<HTMLElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const dirty = acpDirty || configurationDirty || serverEnvironment.dirty || mobile && (notificationDirty
    || !!settings && (notesBase !== settings.notes_base || nodeAddresses !== (settings.node_addresses ?? []).join('\n')
      || executionPath !== (settings.execution_path ?? []).join('\n') || editorWordWrap !== (settings.editor?.word_wrap ?? false))
    || JSON.stringify([agentId, agentType, displayName, command, args.text, env.text]) !== agentBaseline);
  const onClose = () => {
    if (busy || settingsBusy) return;
    if (!dirty || window.confirm(t("settings.settings_have_unsaved_changes_discard_and_close"))) closeDialog();
  };
  useEffect(() => {
    if (closeRequest) closeRequest.current = onClose;
  });
  const selectSection = (next: Section) => {
    // These panels own their drafts and are unmounted when switching categories.
    if (mobile && section !== next && notificationDirty && !window.confirm(t("settings.current_settings_have_unsaved_changes_discard_and_switch"))) return;
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
      setError(t("settings.the_notes_root_must_be_an_absolute_path_on"));
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
      setError(t("settings.enter_one_absolute_server_directory_per_path_line_without"));
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

  const selectDefaultAgent = async (id: string) => {
    setBusy(true); setError('');
    try {
      await agentsApi.setDefaultAgent(id || null);
      await onReload();
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
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

  const closeAgentDialog = () => {
    if (busy) return;
    const changed = JSON.stringify([agentId, agentType, displayName, command, args.text, env.text]) !== agentBaseline;
    if (changed && !window.confirm(t("settings.agent_has_unsaved_changes_discard_and_close"))) return;
    setAgentDialog(undefined); resetAgentForm();
  };

  const loadAgent = (agent: AowAgent, copy = false) => {
    const name = copy ? t("settings.copy", { value0: agent.display_name }) : agent.display_name;
    const nextArgs = argumentsDraft(agent.args);
    const nextEnv = environmentDraft(agent.env);
    setAgentDialog(copy ? 'copy' : 'edit');
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

  const reorderAgents = async (type: AowAgent['agent_type'], ids: string[]) => {
    setBusy(true);
    try {
      await agentsApi.reorderAgents(type, ids);
      await onReload();
    } finally {
      setBusy(false);
    }
  };

  const reloadAgentList = async () => {
    setBusy(true); setAgentRefreshError('');
    try { await onReload(); }
    catch (reason) { setAgentRefreshError(message(reason)); }
    finally { setBusy(false); }
  };

  const save = async () => {
    if (!/^[A-Za-z0-9_-]{1,80}$/.test(agentId)) {
      setError(t("settings.agent_id_is_required_1_80_letters_digits_hyphens"));
      return;
    }
    if (agents.some(agent => agent.id === agentId && agent.id !== editingAgentId)) {
      setError(t("settings.agent_id_already_exists_use_another_id"));
      return;
    }
    if (!agentType) {
      setError(t("settings.choose_an_agent_type"));
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
      setAgentDialog(undefined);
      setAgentSaved(t("settings.configuration_saved", { value0: displayName }));
      await reloadAgentList();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  };

  return <div className={embedded ? 'project-aow-settings-page' : mobile ? 'mobile-settings-backdrop' : 'project-aow-modal-backdrop'} onPointerDown={embedded ? undefined : onClose}>
    <section ref={dialog} tabIndex={-1} className={`project-aow-modal project-aow-dialog project-aow-settings${mobile ? ' mobile-settings' : ''}`} role={embedded ? 'region' : 'dialog'} aria-modal={embedded ? undefined : true} aria-labelledby="aow-settings-title" onPointerDown={(event) => event.stopPropagation()}
      onKeyDown={event => {
        if (embedded) return;
        if (event.key === 'Escape') { event.stopPropagation(); if (mobile && !overview && !busy && !settingsBusy) setOverview(true); else onClose(); }
        if (event.key === 'Tab') {
          const controls = [...event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], summary, [tabindex="0"]')].filter(element => element.getClientRects().length > 0);
          const first = controls[0]; const last = controls.at(-1);
          if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog.current)) { event.preventDefault(); last?.focus(); }
          else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
        }
      }}>
      {mobile ? <header className="mobile-settings-header">
        <button type="button" className="mobile-icon-button" aria-label={overview ? t("settings.back_to_home") : t("settings.back_to_settings_categories")} disabled={busy || settingsBusy} onClick={() => { if (overview) onClose(); else setOverview(true); }}><ArrowLeft /></button>
        <strong id="aow-settings-title">{overview ? t("settings.settings") : sections.find(item => item.id === section)?.label}</strong>
        {!overview && <button type="button" className="mobile-icon-button" aria-label={t("settings.close_settings")} disabled={busy || settingsBusy} onClick={onClose}><X /></button>}
      </header> : <header><div><Settings /><strong id="aow-settings-title">{t("settings.settings")}</strong></div><button type="button" title={t("aow.close")} aria-label={t("aow.close")} disabled={busy || settingsBusy} onClick={onClose}><X /></button></header>}
      <div className="project-aow-settings-body">
        <nav className="project-aow-settings-nav" aria-label={t("settings.settings_categories")} hidden={mobile && !overview}>
          {sections.map(({ id, label, description, icon: Icon }) => <button key={id} type="button" disabled={busy || settingsBusy} aria-current={!overview && section === id ? 'page' : undefined} className={!overview && section === id ? 'active' : ''} onClick={() => selectSection(id)}>
            <Icon /><span><strong>{label}</strong><small>{description}</small></span>
            {mobile && <ChevronRight />}
          </button>)}
          {!mobile && <LogoutButton disabled={busy || settingsBusy} />}
        </nav>
        <div className="project-aow-settings-content" hidden={mobile && overview}>
          <div className="git-platform-provider-settings-host" hidden={section !== 'git-platforms'}><GitPlatformProviderSettings active={section === 'git-platforms'} onBusyChange={setSettingsBusy} /></div>
          <div className="configuration-settings-host" hidden={section !== 'configuration'}><ConfigurationSettings active={section === 'configuration'} onBusyChange={setSettingsBusy} onDirtyChange={setConfigurationDirty} /></div>
          <div className="configuration-settings-host" hidden={section !== 'acp'}><AcpSettings active={section === 'acp'} onBusyChange={setSettingsBusy} onDirtyChange={setAcpDirty} /></div>
          {section === 'git-platforms' || section === 'configuration' || section === 'acp' ? null : section === 'appearance' ? <AppearanceSettings /> : section === 'nodes' ? (
            <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); void saveNodes(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>{t("navigation.nodes")}</h2><p>{t("settings.configure_aow_deployments_on_other_machines_click_the_logo")}</p></div></div>
                <label className="project-aow-dialog-field"><span>{t("settings.node_addresses_one_per_line")}</span><textarea aria-label={t("settings.node_addresses")} spellCheck={false} rows={10} value={nodeAddresses} disabled={settingsLoading || settingsBusy || !settings} onChange={event => { setNodeAddresses(event.target.value); setNodesSaved(false); setError(''); }} placeholder={'https://aow-a.example.com\nhttp://192.168.1.20:8080'} /></label>
                <p className="project-aow-form-intro">{t("settings.enter_full_http_or_https_urls_optionally_including_this")}</p>
                {nodesSaved ? <p role="status">{t("settings.node_addresses_saved")}</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !settings}>{settingsBusy ? t("automations.saving") : t("acp.save")}</button></footer>
            </form>
          ) : section === 'im' || section === 'notifications' ? <NotificationSettingsPanel section={section} onBusyChange={setSettingsBusy} onDirtyChange={setNotificationDirty} /> : section === 'editor' ? (
            <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); void saveEditor(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>{t("navigation.editor")}</h2><p>{t("settings.set_global_defaults_for_the_file_editor")}</p></div></div>
                <label className="project-aow-dialog-checkbox"><input type="checkbox" checked={editorWordWrap} disabled={settingsLoading || settingsBusy || !settings} onChange={event => { setEditorWordWrap(event.target.checked); setEditorSaved(false); setError(''); }} /><span>{t("settings.word_wrap")}</span></label>
                <p className="project-aow-form-intro">{t("settings.wrap_long_lines_to_fit_the_editor_width_each")}</p>
                {editorSaved ? <p role="status">{t("settings.editor_configuration_saved")}</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !settings}>{settingsBusy ? t("automations.saving") : t("acp.save")}</button></footer>
            </form>
          ) : section === 'notes' ? <>
            <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void saveNotesRoot(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>{t("navigation.notes")}</h2><p>{t("settings.new_projects_without_a_notes_path_map_their_repository")}</p></div></div>
                <label className="project-aow-dialog-field"><span>{t("navigation.notes_root")}</span><input className="project-aow-dialog-monospace" autoFocus={!mobile} spellCheck={false} value={notesBase} disabled={settingsLoading || settingsBusy} onChange={(event) => { setNotesBase(event.target.value); setNotesSaved(false); setError(''); }} placeholder="/absolute/path/to/aow" required /></label>
                <small className="project-aow-dialog-path-hint">{t("settings.current_root")}<code title={settings?.notes_base}>{settings?.notes_base ?? t("automations.loading")}</code></small>
                <p className="project-aow-form-intro">{t("settings.only_the_default_root_changes_notes_are_not_moved")}</p>
                {notesSaved ? <p role="status">{t("settings.notes_root_saved_existing_projects_keep_their_notes_paths")}</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !notesBase.trim()}>{settingsBusy ? t("automations.saving") : t("acp.save")}</button></footer>
            </form>
          </> : section === 'environment' ? <>
            <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void saveEnvironment(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>{t("navigation.environment")}</h2><p>{t("settings.configure_execution_path_and_edit_the_aow_service_environment")}</p></div></div>
                <div className="project-aow-settings-heading environment-settings-heading">
                  <div><h3>{t("settings.path_directories_highest_priority_first")}</h3><p>{t("settings.one_absolute_server_directory_per_line_put_the_directory")}</p></div>
                  <div className="environment-settings-buttons">
                    <button type="button" className="project-aow-dialog-button" disabled={settingsLoading || settingsBusy} onClick={() => void readEnvironment()}><RefreshCw size={14} />{t("settings.read_from_local_environment")}</button>
                    <button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !executionPath.trim()}>{settingsBusy ? t("acp.processing") : t("settings.save_path")}</button>
                  </div>
                </div>
                <div className="project-aow-dialog-field"><textarea aria-label={t("settings.path_directories")} spellCheck={false} rows={10} value={executionPath} disabled={settingsLoading || settingsBusy} onChange={(event) => { setExecutionPath(event.target.value); setEnvironmentSaved(false); setError(''); }} placeholder={'/opt/python/3.11/bin\n/usr/local/bin\n/usr/bin\n/bin'} required /></div>
                <p className="project-aow-form-intro">{t("settings.saved_settings_apply_to_future_executions_without_resaving_tasks")}</p>
                {environmentSaved ? <p role="status">{t("settings.execution_environment_saved")}</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
                <ServerEnvironmentFields environment={serverEnvironment} busy={settingsBusy} />
              </div>
            </form>
          </> : <>
            <div className="project-aow-dialog-form"><div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>{t("navigation.agents")}</h2><p>{t("settings.supports_claude_code_codex_traecode_cli_hermes_and_pi")}</p></div><button type="button" className="project-aow-dialog-button" title={t("settings.detect_local_agents_again")} disabled={busy} onClick={() => void reloadAgentList()}><RefreshCw />{t("files.refresh")}</button><button type="button" className="project-aow-dialog-button primary" disabled={busy} onClick={() => { resetAgentForm(); setAgentDialog('new'); }}>{t("settings.add_agent")}</button></div>
                {agentsError && <div className="project-aow-error" role="alert">{agentsError}</div>}
                <AgentList agents={agents} busy={busy} editingAgentId={editingAgentId} onEdit={loadAgent} onCopy={agent => loadAgent(agent, true)} onRemove={id => void removeAgent(id)} onReorder={reorderAgents} onDefaultChange={id => void selectDefaultAgent(id)} />
                {agentSaved && <p role="status">{agentSaved}</p>}
                {agentRefreshError && <div className="project-aow-error" role="alert">{t("settings.failed_to_refresh_list")}{agentRefreshError}<button type="button" className="project-aow-dialog-button" disabled={busy} onClick={() => void reloadAgentList()}>{t("settings.retry_refresh")}</button></div>}
                {!agentDialog && error && <div className="project-aow-error" role="alert">{error}</div>}
              </div></div>
            {agentDialog && <SettingsActionDialog title={agentDialog === 'edit' ? t("settings.edit_agent_configuration") : agentDialog === 'copy' ? t("settings.copy_agent_configuration") : t("settings.add_agent")} busy={busy} onClose={closeAgentDialog}>
              <form className="project-aow-dialog-form" ref={agentForm} onSubmit={event => { event.preventDefault(); event.stopPropagation(); void save(); }}>
                <div className="project-aow-dialog-body">
                <label className="project-aow-dialog-field"><span>Agent ID <em>{t("settings.required")}</em></span><input aria-label="Agent ID" value={agentId} readOnly={!!editingAgentId} disabled={busy} onChange={event => { setAgentId(event.target.value); setAgentSaved(''); setError(''); }} placeholder={t("settings.for_example_work_codex")} pattern={'[A-Za-z0-9_\\-]+'} maxLength={80} autoCapitalize="none" spellCheck={false} required /><small>{t("settings.unique_identifier_using_letters_digits_hyphens_and_underscores_immutable")}</small></label>
                <label className="project-aow-dialog-field"><span>{t("settings.agent_type")} <em>{t("settings.required")}</em></span><div className="project-aow-agent-type"><AgentIcon agentId={agentType} /><select aria-label={t("settings.agent_type")} value={agentType ?? ''} disabled={busy || !!builtinAgentType(editingAgentId)} onChange={(event) => selectAgentType(event.target.value)} required><option value="" disabled>{t("settings.choose_agent_type")}</option>{agentTypes.map(type => <option key={type.id} value={type.id}>{type.label}</option>)}</select></div></label>
                <p className="project-aow-form-intro">{t("settings.selecting_a_type_fills_the_detected_executable_path_or")}</p>
                <label className="project-aow-dialog-field"><span>{t("navigation.display_name")}</span><input value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} placeholder={t("settings.for_example_work_codex_2")} required /></label>
                <label className="project-aow-dialog-field"><span>{t("navigation.executable")}</span><input className="project-aow-dialog-monospace" spellCheck={false} value={command} disabled={busy} onChange={(event) => setCommand(event.target.value)} placeholder={t("settings.command_name_or_absolute_path")} required /></label>
                <AgentArgumentsInput value={args} onChange={setArgs} executable={command} disabled={busy} onError={setError} />
                <label className="project-aow-dialog-field"><span>{t("navigation.environment_variables")}</span><textarea aria-label={t("navigation.environment_variables")} spellCheck={false} rows={4} value={env.text} disabled={busy} onChange={(event) => setEnv({ ...env, text: event.target.value })} placeholder={'BASE_URL=https://example.com\nAPI_KEY=your-key'} /></label>
                <p className="project-aow-form-intro">{t("settings.inherits_the_launch_environment_enter_key_value_per_line")}</p>
                <p className="project-aow-form-intro">{t("settings.saved_settings_apply_to_newly_started_terminal_agents_configure")}</p>
                  {error && <div className="project-aow-error" role="alert">{error}</div>}
                </div>
                <footer className="project-aow-dialog-footer"><button type="button" className="project-aow-dialog-button" disabled={busy} onClick={closeAgentDialog}>{t("aow.cancel")}</button><button type="submit" className="project-aow-dialog-button primary" disabled={busy || !agentType}>{busy ? t("automations.saving") : editingAgentId ? t("settings.save_configuration") : t("settings.register")}</button></footer>
              </form>
            </SettingsActionDialog>}
          </>}
        </div>
      </div>
    </section>
  </div>;
}
