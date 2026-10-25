import { t } from '../../../lib/i18n';
import { lazy, Suspense, useEffect, useMemo, useState } from 'react';
import { applyEdits, format, modify, parse, printParseErrorCode, type ParseError } from 'jsonc-parser';
import { useConfirmation } from '../../../components/ConfirmationDialog';
import { SettingsActionDialog } from '../../settings/SettingsActionDialog';
import { acpApi } from '../api';
import { Download } from 'lucide-react';
import { AcpRegistry } from './AcpRegistry';
import { AgentIcon } from '../../agents/AgentIcon';
import { failure, object, type AgentInfo, type SettingsFile } from '../types';
import { ConnectionProgress } from '../agent_ui/connection_progress';
import { useProgress } from '../agent_ui/use_progress';
import '../acp.css';

const MonacoEditor = lazy(() => import('../../editor/MonacoEditor'));
const jsoncFormattingOptions = { insertSpaces: true, tabSize: 2, eol: '\n' };
const jsoncSchemaUri = 'aow://schemas/acp-settings.jsonc';
const jsoncModelMatch = '**/aow-acp-settings.jsonc';

function parseJsonc(content: string) {
  const errors: ParseError[] = [];
  const value: unknown = parse(content, errors, { allowTrailingComma: true });
  return { value, errors };
}

function syntaxErrorMessage(content: string, error: ParseError) {
  const offset = Math.min(error.offset, content.length);
  const beforeError = content.slice(0, offset);
  const line = beforeError.split('\n').length;
  const column = offset - beforeError.lastIndexOf('\n');
  const errorName = printParseErrorCode(error.error);
  const messages: Record<string, string> = {
    InvalidSymbol: t("acp.invalid_symbol"),
    InvalidNumberFormat: t("acp.invalid_number_format"),
    PropertyNameExpected: t("acp.property_name_expected"),
    ValueExpected: t("acp.value_expected"),
    ColonExpected: t("acp.colon_expected_after_property_name"),
    CommaExpected: t("acp.comma_expected"),
    CloseBraceExpected: t("acp.closing_brace_missing"),
    CloseBracketExpected: t("acp.closing_bracket_missing"),
    EndOfFileExpected: t("acp.unexpected_content_after_object_or_array"),
    InvalidCommentToken: t("acp.invalid_comment"),
    UnexpectedEndOfComment: t("acp.unterminated_block_comment"),
    UnexpectedEndOfString: t("acp.unterminated_string"),
    UnexpectedEndOfNumber: t("acp.incomplete_number"),
    InvalidUnicode: t("acp.invalid_unicode_escape"),
    InvalidEscapeCharacter: t("acp.invalid_string_escape"),
    InvalidCharacter: t("acp.invalid_character"),
  };
  return t("acp.jsonc_syntax_error_line_column", { value0: line, value1: column, value2: messages[errorName] ?? t("common.invalid_format") });
}

function formatJsonc(content: string) {
  return applyEdits(content, format(content, undefined, jsoncFormattingOptions));
}

function agentSettings(content: string) {
  const { value, errors } = parseJsonc(content);
  if (errors.length) throw new Error(syntaxErrorMessage(content, errors[0]));
  return object(object(value).agent_servers);
}
export function AcpSettings({ active, onBusyChange, onDirtyChange }: { active: boolean; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void }) {
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
  const [modal, setModal] = useState<'registry' | 'edit'>();
  const [modalBaseline, setModalBaseline] = useState('');
  const { confirm, confirmationDialog } = useConfirmation();
  const dirty = !!saved && content !== saved.content;
  const parsed = useMemo(() => parseJsonc(content), [content]);
  const syntaxErrors = parsed.errors;
  const defaultAgentId = String(object(parsed.value).default_agent_id ?? '');
  const syntaxError = syntaxErrors[0] ? syntaxErrorMessage(content, syntaxErrors[0]) : '';
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
    if (!saved) throw new Error(t("acp.acp_configuration_has_not_loaded_yet"));
    const { errors } = parseJsonc(draft);
    if (errors.length) throw new Error(syntaxErrorMessage(draft, errors[0]));
    const next = await acpApi.saveSettings(formatJsonc(draft), saved.revision);
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
      setStatus(t("acp.installed_create_a_session_from_the_acp_group_in", { value0: id }));
    } catch (reason) { setFailed(values => [...new Set([...values, id])]); throw reason; }
    finally {
      setInstalling('');
      await acpApi.agents().then(setAgents).catch(reason => setError(failure(reason)));
    }
  });
  const closeModal = async () => {
    if (busy) return;
    if (modal === 'edit') {
      if (content !== modalBaseline && !await confirm({
        title: t("acp.discard_acp_changes"),
        description: t("acp.acp_has_unsaved_changes_discard_and_close"),
        confirmLabel: t("acp.discard_changes"),
        danger: true,
      })) return;
      setContent(modalBaseline);
    }
    setModal(undefined); setError('');
  };
  const openEditor = (mode: 'edit') => { setModalBaseline(content); setModal(mode); setError(''); setStatus(''); };
  const saveEditor = () => run(async () => {
    const next = await save(content);
    setModalBaseline(next.content);
    setAgents(await acpApi.agents());
    setModal(undefined);
    setStatus(t("acp.saved_new_connections_will_use_the_updated_configuration"));
  });
  const selectDefaultAgent = (id: string) => run(async () => {
    await save(applyEdits(content, modify(content, ['default_agent_id'], id || undefined, { formattingOptions: jsoncFormattingOptions })));
    setAgents(await acpApi.agents());
    setStatus(t("acp.default_acp_instance_saved"));
  });
  const installButton = (agent: AgentInfo, registryEntry: boolean) => {
    const installed = agents.some(value => value.id === agent.id && value.installed);
    return <button type="button" disabled={busy || !agent.supported || installed} onClick={() => { void install(agent.id); }}>
      {registryEntry && !installed && <Download size={14} aria-hidden="true" />}{installing === agent.id ? (registryEntry ? t("acp.installing") : t("acp.installing")) : installed ? (registryEntry ? t("acp.installed") : t("acp.installed")) : failed.includes(agent.id) ? (registryEntry ? t("aow.retry") : t("acp.retry_installation")) : registryEntry ? t("acp.install") : t("acp.install")}
    </button>;
  };
  return <div className="project-aow-dialog-form acp-settings">
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading acp-settings-heading"><div><h2>ACP</h2><p>{t("acp.install_a_registry_agent_or_configure_a_custom_launch")}</p></div><div className="acp-actions"><button type="button" disabled={busy || !saved} onClick={() => { setModal('registry'); void run(async () => setRegistry(await acpApi.registry(true))); }}>ACP Registry</button><button type="button" disabled={busy || !saved} onClick={() => openEditor('edit')}>{t("navigation.setting_file")}</button></div></div>
      {!!agents.length && <div className="acp-registry" aria-label={t("acp.configured_acp_agents")}>{agents.map(agent => <article key={agent.id}>
        <AgentIcon agentId={agent.id.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} />
        <div><strong>{agent.name}</strong>{!agent.installed && <small>{t("acp.not_installed_complete_installation_first")}</small>}</div>
        <label className="project-aow-agent-default" title={defaultAgentId === agent.id ? t("acp.clear_default_and_use_the_first_item") : t("acp.set_as_default_instance")}>
          <input type="checkbox" aria-label={t("acp.default_acp", { value0: agent.name })} checked={defaultAgentId === agent.id} disabled={busy || !saved || !!syntaxError} onChange={event => { void selectDefaultAgent(event.target.checked ? agent.id : ''); }} />{t("aow.default")}</label>
        <button type="button" disabled={busy || !saved} onClick={() => openEditor('edit')} aria-label={t("acp.edit_acp_configuration", { value0: agent.name })}>{t("acp.edit")}</button>{installButton(agent, false)}
      </article>)}</div>}
      {!agents.length && saved && <p className="project-aow-empty">{t("acp.no_acp_agents_configured")}</p>}

      {modal === 'registry' && <SettingsActionDialog title="ACP Registry" busy={busy} onClose={closeModal}>
        <div className="project-aow-dialog-body">
          {registry && <AcpRegistry registry={registry} agents={agents} installButton={installButton} />}
          {!registry && !error && <p role="status">{t("acp.loading_acp_registry")}</p>}
          {installing && <ConnectionProgress status={progress} message={t("acp.installing_2", { value0: installing })} />}
          {error && <p className="acp-error" role="alert">{error}</p>}{status && <p role="status">{status}</p>}
        </div>
        <footer className="project-aow-dialog-footer"><button type="button" disabled={busy} onClick={closeModal}>{t("aow.close")}</button></footer>
      </SettingsActionDialog>}
      {(modal === 'edit') && <SettingsActionDialog title={t("acp.edit_acp_configuration_2")} busy={busy} onClose={closeModal}>
        <form className="project-aow-dialog-form acp-settings" onSubmit={event => { event.preventDefault(); event.stopPropagation(); void saveEditor(); }}>
          <div className="project-aow-dialog-body">
      <div className="project-aow-dialog-field">
        <div className="acp-settings-editor-toolbar"><span id="acp-settings-editor-label">{saved?.path ?? 'aow-acp.json'}</span><button type="button" disabled={busy || !saved || !!syntaxError} onClick={() => { setContent(formatJsonc(content)); setError(''); setStatus(t("acp.jsonc_formatted")); }}>{t("acp.format")}</button></div>
        <div className="acp-settings-editor" role="group" aria-labelledby="acp-settings-editor-label">
          {active && saved ? <Suspense fallback={<div className="acp-settings-editor-loading">{t("aow.loading_editor")}</div>}>
            <MonacoEditor
              language="json"
              path="inmemory://aow/acp/aow-acp-settings.jsonc"
              value={content}
              onChange={value => setContent(value ?? '')}
              onMount={(_editor, monaco) => {
                const defaults = monaco.json.jsonDefaults;
                const diagnostics = defaults.diagnosticsOptions;
                if (!diagnostics.schemas?.some((schema: { uri: string }) => schema.uri === jsoncSchemaUri)) {
                  defaults.setDiagnosticsOptions({
                    ...diagnostics,
                    schemas: [...(diagnostics.schemas ?? []), {
                      uri: jsoncSchemaUri,
                      fileMatch: [jsoncModelMatch],
                      schema: { type: 'object', allowComments: true, allowTrailingCommas: true },
                    }],
                  });
                }
              }}
              options={{
                ariaLabel: t("acp.acp_jsonc_configuration"),
                automaticLayout: true,
                readOnly: busy,
                minimap: { enabled: false },
                fontSize: 12,
                lineHeight: 19,
                tabSize: 2,
                insertSpaces: true,
                formatOnPaste: true,
                formatOnType: true,
                scrollBeyondLastLine: false,
                padding: { top: 8, bottom: 8 },
                wordWrap: 'off',
              }}
            />
          </Suspense> : <div className="acp-settings-editor-loading">{t("acp.loading_acp_configuration")}</div>}
        </div>
      </div>
      <small>{t("acp.comments_and_trailing_commas_are_supported_new_connections_use")}</small>
      {syntaxError && <p className="acp-error" role="alert">{syntaxError}</p>}
      {error && <p className="acp-error" role="alert">{error}</p>}
          </div>
          <footer className="project-aow-dialog-footer"><button type="button" disabled={busy} onClick={closeModal}>{t("aow.cancel")}</button><button type="submit" className="project-aow-dialog-button primary" disabled={busy || !saved || !dirty || !!syntaxError}>{t("acp.save")}</button></footer>
        </form>
      </SettingsActionDialog>}
      {!modal && installing && <ConnectionProgress status={progress} message={t("acp.installing_2", { value0: installing })} />}
      {!modal && error && <p className="acp-error" role="alert">{error}</p>}{!modal && status && <p role="status">{status}</p>}
    </div>
    {confirmationDialog}
  </div>;
}
