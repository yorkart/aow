import { useEffect, useState } from 'react';
import { listConnectors } from '../connectors/api';
import { executorNames, providerNames, type Connector } from '../connectors/types';
import { checkRuntime, getRuntimeOptions, saveRuntime } from './api';
import { useDirty, useOperation } from './hooks';
import type { Navigate, PanelProps, RuntimeOptions } from './types';

export function AgentRuntime({ org, agent, onSaved, onDirtyChange, onNavigate }: PanelProps & { onNavigate: Navigate }) {
  const [connector, setConnector] = useState(agent.connector.id); const [runtime, setRuntime] = useState(agent.runtime);
  const [connectors, setConnectors] = useState<Connector[]>(); const [options, setOptions] = useState<RuntimeOptions>();
  const [loadError, setLoadError] = useState(''); const [retry, setRetry] = useState(0); const [check, setCheck] = useState<{ status: string; message: string }>();
  const dirty = connector !== agent.connector.id || JSON.stringify(runtime) !== JSON.stringify(agent.runtime); useDirty(dirty, onDirtyChange);
  const { busy, error, perform } = useOperation();
  useEffect(() => { const abort = new AbortController(); void listConnectors(org, abort.signal).then(setConnectors).catch((error: unknown) => { if (!abort.signal.aborted) setLoadError(error instanceof Error ? error.message : '无法读取连接。'); }); return () => abort.abort(); }, [org, retry]);
  useEffect(() => {
    const abort = new AbortController();
    void getRuntimeOptions(org, connector, abort.signal).then(value => { if (!abort.signal.aborted) setOptions(value); }).catch((error: unknown) => { if (!abort.signal.aborted) setLoadError(error instanceof Error ? error.message : '无法读取运行选项。'); });
    return () => abort.abort();
  }, [org, connector, retry]);
  const selected = connectors?.find(item => item.id === connector); const model = options?.models.find(item => item.id === runtime.model);
  const invalidModel = runtime.model !== null && !model; const invalidReasoning = runtime.reasoning_effort !== null && !model?.reasoning_efforts.includes(runtime.reasoning_effort);
  const ready = options?.connector.id === connector && selected?.status === 'available';
  return <><header className="agent-panel-heading"><div><h2>Harness / Runtime</h2><p className="agent-note">认证由 Connector 管理。运行时使用任务指定的项目工作目录。</p></div><button type="button" onClick={() => onNavigate('connectors', connector)}>配置 Connector</button></header>
    {loadError && <p className="agent-error" role="alert">{loadError}<button type="button" onClick={() => { setLoadError(''); setRetry(value => value + 1); }}>重试</button></p>}
    <form className="agent-form agent-runtime-form" onSubmit={event => { event.preventDefault(); void perform(async () => onSaved(await saveRuntime(org, agent, connector, runtime))); }}>
      <label>Connector<select aria-label="Agent Connector" disabled={busy || !connectors} value={connector} onChange={event => { setConnector(event.target.value); setOptions(undefined); setRuntime({ model: null, reasoning_effort: null, timeout_seconds: runtime.timeout_seconds }); setCheck(undefined); setLoadError(''); }}>{(connectors ?? [agent.connector]).filter(item => item.status === 'available' || item.id === connector).map(item => <option key={item.id} value={item.id}>{item.name}{item.status === 'available' ? '' : '（不可用）'}</option>)}</select></label>
      <p className="agent-note">{selected?.account_summary || agent.connector.account_summary}</p><dl><dt>Provider / 执行器</dt><dd>{selected ? `${providerNames[selected.provider]} / ${executorNames[selected.executor]}` : '正在读取…'}</dd></dl>
      <label>Model<select aria-label="Agent Model" disabled={busy || !ready} value={runtime.model ?? ''} onChange={event => { setRuntime({ ...runtime, model: event.target.value || null, reasoning_effort: null }); setCheck(undefined); }}><option value="">Default（执行器默认）</option>{options?.models.map(model => <option value={model.id} key={model.id}>{model.id}</option>)}{invalidModel && <option value={runtime.model!}>{runtime.model}（需要重新选择）</option>}</select></label>
      {Boolean(model?.reasoning_efforts.length || invalidReasoning) && <label>推理强度<select aria-label="推理强度" disabled={busy} value={runtime.reasoning_effort ?? ''} onChange={event => { setRuntime({ ...runtime, reasoning_effort: event.target.value || null }); setCheck(undefined); }}><option value="">Default</option>{model?.reasoning_efforts.map(level => <option key={level} value={level}>{level}</option>)}{invalidReasoning && <option value={runtime.reasoning_effort!}>原设置已不兼容，请重新选择</option>}</select></label>}
      <label>运行超时（秒）<input type="number" min={options?.timeout_min ?? 30} max={options?.timeout_max ?? 86400} required disabled={busy} value={runtime.timeout_seconds} onChange={event => { setRuntime({ ...runtime, timeout_seconds: Number(event.target.value) }); setCheck(undefined); }} /></label>
      {(invalidModel || invalidReasoning) && <p className="agent-error" role="alert">原模型或参数已不兼容，请重新选择。</p>}{error && <p className="agent-error" role="alert">{error}</p>}{check && <p className={check.status === 'available' ? 'agent-check' : 'agent-error'} role="status">{check.message}</p>}
      <footer className="agent-actions"><button type="button" disabled={busy || !ready || invalidModel || invalidReasoning} onClick={() => { void perform(async () => setCheck(await checkRuntime(org, agent, connector, runtime))); }}>检查运行环境</button><button className="orbit-primary" type="submit" aria-busy={busy} disabled={busy || !dirty || !ready || invalidModel || invalidReasoning}>保存运行配置</button></footer>
    </form></>;
}
