import { lazy, Suspense, useEffect, useMemo, useState } from 'react';
import { applyEdits, format, modify, parse, printParseErrorCode, type ParseError } from 'jsonc-parser';
import { useConfirmation } from '../../../components/ConfirmationDialog';
import { SettingsActionDialog } from '../../settings/SettingsActionDialog';
import { acpApi } from '../api';
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
    InvalidSymbol: '存在无效符号',
    InvalidNumberFormat: '数字格式无效',
    PropertyNameExpected: '此处需要属性名',
    ValueExpected: '此处需要值',
    ColonExpected: '属性名后需要冒号',
    CommaExpected: '此处需要逗号',
    CloseBraceExpected: '缺少右花括号 }',
    CloseBracketExpected: '缺少右方括号 ]',
    EndOfFileExpected: '对象或数组后存在多余内容',
    InvalidCommentToken: '注释格式无效',
    UnexpectedEndOfComment: '块注释未闭合',
    UnexpectedEndOfString: '字符串未闭合',
    UnexpectedEndOfNumber: '数字未完成',
    InvalidUnicode: 'Unicode 转义无效',
    InvalidEscapeCharacter: '字符串转义无效',
    InvalidCharacter: '包含无效字符',
  };
  return `JSONC 语法错误（第 ${line} 行，第 ${column} 列）：${messages[errorName] ?? '格式无效'}。`;
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
  const [modal, setModal] = useState<'registry' | 'custom' | 'edit'>();
  const [modalBaseline, setModalBaseline] = useState('');
  const [search, setSearch] = useState('');
  const { confirm, confirmationDialog } = useConfirmation();
  const dirty = !!saved && content !== saved.content;
  const parsed = useMemo(() => parseJsonc(content), [content]);
  const syntaxErrors = parsed.errors;
  const configuredIds = Object.keys(object(object(parsed.value).agent_servers)).sort();
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
    if (!saved) throw new Error('ACP 配置尚未加载。');
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
      setStatus(`${id} 已安装，可在工作区“新建窗体”菜单的 ACP 分组中新建会话。`);
    } catch (reason) { setFailed(values => [...new Set([...values, id])]); throw reason; }
    finally {
      setInstalling('');
      await acpApi.agents().then(setAgents).catch(reason => setError(failure(reason)));
    }
  });
  const closeModal = async () => {
    if (busy) return;
    if (modal === 'custom' || modal === 'edit') {
      if (content !== modalBaseline && !await confirm({
        title: '放弃 ACP 修改？',
        description: 'ACP 有未保存的修改，是否放弃并关闭？',
        confirmLabel: '放弃修改',
        danger: true,
      })) return;
      setContent(modalBaseline);
    }
    setModal(undefined); setError('');
  };
  const openEditor = (mode: 'custom' | 'edit') => { setModalBaseline(content); setModal(mode); setError(''); setStatus(''); };
  const saveEditor = () => run(async () => {
    const next = await save(content);
    setModalBaseline(next.content);
    setAgents(await acpApi.agents());
    setModal(undefined);
    setStatus('已保存，新连接将使用更新后的配置。');
  });
  const selectDefaultAgent = (id: string) => run(async () => {
    await save(applyEdits(content, modify(content, ['default_agent_id'], id || undefined, { formattingOptions: jsoncFormattingOptions })));
    setAgents(await acpApi.agents());
    setStatus('默认 ACP 实例已保存。');
  });
  const installButton = (agent: AgentInfo, registryEntry: boolean) => {
    const installed = agents.some(value => value.id === agent.id && value.installed);
    const configured = agents.some(value => value.id === agent.id);
    return <button type="button" disabled={busy || !agent.supported || installed} onClick={() => { void install(agent.id); }}>
      {installing === agent.id ? '安装中…' : installed ? '已安装' : failed.includes(agent.id) ? '重试安装' : configured || !registryEntry ? '安装' : '添加并安装'}
    </button>;
  };
  return <div className="project-aow-dialog-form acp-settings">
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>ACP</h2><p>安装 Registry Agent 或配置自定义启动命令。</p></div></div>
      <label className="project-aow-dialog-field">默认 ACP 实例
        <select aria-label="默认 ACP 实例" disabled={busy || !saved || !!syntaxError} value={defaultAgentId} onChange={event => { void selectDefaultAgent(event.target.value); }}>
          <option value="">使用列表第一项</option>
          {configuredIds.map(id => <option key={id} value={id}>{id}</option>)}
        </select>
      </label>
      {!!agents.length && <div className="acp-registry" aria-label="已配置的 ACP Agent">{agents.map(agent => <article key={agent.id}><div><strong>{agent.name}</strong><small>{agent.installed ? '可用于新建会话' : '尚未安装，请先完成安装'}</small></div><button type="button" disabled={busy || !saved} onClick={() => openEditor('edit')} aria-label={`编辑 ${agent.name} ACP 配置`}>编辑</button>{installButton(agent, false)}</article>)}</div>}
      {!agents.length && saved && <p className="project-aow-empty">尚未配置 ACP Agent。</p>}
      <div className="acp-actions"><button type="button" disabled={busy || !saved} onClick={() => { setModal('registry'); setSearch(''); void run(async () => setRegistry(await acpApi.registry(true))); }}>浏览 ACP Registry</button><button type="button" disabled={busy || !saved} onClick={() => openEditor('custom')}>添加自定义 Agent</button></div>
      {modal === 'registry' && <SettingsActionDialog title="ACP Registry" busy={busy} onClose={closeModal}>
        <div className="project-aow-dialog-body">
      {registry && <div className="acp-registry" aria-label="ACP Registry"><input aria-label="搜索 ACP Registry" placeholder="搜索 Agent…" value={search} onChange={event => setSearch(event.target.value)} />{registry.filter(agent => `${agent.id} ${agent.name} ${agent.description}`.toLowerCase().includes(search.toLowerCase())).map(agent => <article key={agent.id}><div><strong>{agent.name}</strong><small>{agent.description}</small></div>{installButton(agent, true)}</article>)}</div>}
          {!registry && !error && <p role="status">正在加载 ACP Registry…</p>}
          {installing && <ConnectionProgress status={progress} message={`正在安装 ${installing}…`} />}
          {error && <p className="acp-error" role="alert">{error}</p>}{status && <p role="status">{status}</p>}
        </div>
        <footer className="project-aow-dialog-footer"><button type="button" disabled={busy} onClick={closeModal}>关闭</button></footer>
      </SettingsActionDialog>}
      {(modal === 'custom' || modal === 'edit') && <SettingsActionDialog title={modal === 'custom' ? '添加自定义 ACP Agent' : '编辑 ACP 配置'} busy={busy} onClose={closeModal}>
        <form className="project-aow-dialog-form acp-settings" onSubmit={event => { event.preventDefault(); event.stopPropagation(); void saveEditor(); }}>
          <div className="project-aow-dialog-body">
      <div className="project-aow-dialog-field">
        <div className="acp-settings-editor-toolbar"><span id="acp-settings-editor-label">{saved?.path ?? 'acp/settings.json'}</span><button type="button" disabled={busy || !saved || !!syntaxError} onClick={() => { setContent(formatJsonc(content)); setError(''); setStatus('JSONC 已格式化。'); }}>格式化</button></div>
        <div className="acp-settings-editor" role="group" aria-labelledby="acp-settings-editor-label">
          {active && saved ? <Suspense fallback={<div className="acp-settings-editor-loading">正在加载编辑器…</div>}>
            <MonacoEditor
              language="json"
              path="inmemory://aow/acp/aow-acp-settings.jsonc"
              theme="vs-dark"
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
                ariaLabel: 'ACP JSONC 配置',
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
          </Suspense> : <div className="acp-settings-editor-loading">正在加载 ACP 配置…</div>}
        </div>
      </div>
      <small>支持注释和尾逗号。保存后新连接将使用更新后的配置。</small>
      {syntaxError && <p className="acp-error" role="alert">{syntaxError}</p>}
      {error && <p className="acp-error" role="alert">{error}</p>}
          </div>
          <footer className="project-aow-dialog-footer"><button type="button" disabled={busy} onClick={closeModal}>取消</button><button type="submit" className="project-aow-dialog-button primary" disabled={busy || !saved || !dirty || !!syntaxError}>保存</button></footer>
        </form>
      </SettingsActionDialog>}
      {!modal && installing && <ConnectionProgress status={progress} message={`正在安装 ${installing}…`} />}
      {!modal && error && <p className="acp-error" role="alert">{error}</p>}{!modal && status && <p role="status">{status}</p>}
    </div>
    {confirmationDialog}
  </div>;
}
