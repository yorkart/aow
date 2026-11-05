import { useEffect, useId, useState } from 'react';
import { ArrowLeft, ArrowRight, Check, LoaderCircle } from 'lucide-react';
import { t } from '../../lib/i18n';
import { RoleIdentity } from './RoleIdentity';
import { acpApi, settingsChanged } from '../acp/api';
import type { AgentInfo } from '../acp/types';
import { agentsApi } from '../agents/api';
import { aowAgentType } from '../agents/agentTypes';
import type { AowAgent } from '../agents/types';
import { rolesApi, type AgentRole, type RoleConfig, type RoleRuntime, type IdentitySource } from './api';

export function RoleEditor({ role, onClose, onSaved, onBusyChange, onDirtyChange }: { role?: AgentRole; onClose: () => void; onSaved: () => void; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void }) {
  const [requestId] = useState(() => Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join(''));
  const [step, setStep] = useState(0);
  const [description, setDescription] = useState(role?.description ?? '');
  const [name, setName] = useState(role?.name ?? '');
  const [runtime, setRuntime] = useState<RoleRuntime | undefined>(role?.runtime);
  const [source, setSource] = useState(role?.identity.source ?? '');
  const [presets, setPresets] = useState<IdentitySource[]>([]);
  const [reload, setReload] = useState(0);
  const [instruction, setInstruction] = useState(role?.instruction ?? '');
  const [skills, setSkills] = useState(role?.skills.join('\n') ?? '');
  const [localAgents, setLocalAgents] = useState<AowAgent[]>([]);
  const [acpAgents, setAcpAgents] = useState<AgentInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const titleId = useId();
  const locked = busy;
  const runtimes = [
    ...acpAgents.map(agent => ({
      key: `acp:${agent.id}`, runtime: { mode: 'acp', agent_id: agent.id } as const, name: agent.name,
      available: agent.configured && agent.supported && agent.installed,
      preset: presets.find(item => agent.id.toLowerCase().split(/[^a-z0-9]+/).includes(item.layout)),
    })),
    ...localAgents.map(agent => ({
      key: `cli:${agent.id}`, runtime: { mode: 'cli', agent_id: agent.id } as const, name: agent.display_name,
      available: agent.available, preset: presets.find(item => item.layout === aowAgentType(agent)),
    })),
  ];
  const runtimeKey = runtime ? `${runtime.mode}:${runtime.agent_id}` : '';
  const selectedRuntime = runtimes.find(item => item.key === runtimeKey);
  const preset = selectedRuntime?.preset;
  const runtimeReady = !loading && !!preset && !!selectedRuntime?.available;
  const draft = JSON.stringify([name, description, runtime, source, instruction, skills]);
  const [baseline] = useState(draft);
  const dirty = draft !== baseline;
  useEffect(() => { onBusyChange(busy); return () => onBusyChange(false); }, [busy, onBusyChange]);
  useEffect(() => { onDirtyChange(dirty); return () => onDirtyChange(false); }, [dirty, onDirtyChange]);
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void Promise.all([agentsApi.agents(true), acpApi.agents(), rolesApi.sources()]).then(([agents, acp, sources]) => {
      if (!cancelled) { setLocalAgents(agents); setAcpAgents(acp); setPresets(sources); }
    }).catch(reason => { if (!cancelled) { setLocalAgents([]); setAcpAgents([]); setPresets([]); setError(String(reason)); } }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [reload]);
  useEffect(() => {
    const refresh = () => setReload(value => value + 1);
    window.addEventListener(settingsChanged, refresh);
    return () => window.removeEventListener(settingsChanged, refresh);
  }, []);
  const config = (runtime: RoleRuntime): RoleConfig => ({
    name: name.trim(), description: description.trim(), runtime,
    ...(role ? { home_layout: role.home_layout } : {}), identity: role?.identity ?? { kind: 'local_home', source: source.trim() }, instruction, skills: skills.split('\n').map(value => value.trim()).filter(Boolean),
  });
  const next = () => {
    if (locked || !name.trim() || (step === 1 && !runtimeReady)) return;
    setError('');
    setStep(current => current + 1);
  };
  const save = async () => {
    if (locked || !name.trim() || !runtime || !runtimeReady || (!role && !source.trim())) return;
    setBusy(true); setError('');
    try {
      if (role) await rolesApi.save(config(runtime), role.id);
      else await rolesApi.testAndCreate(config(runtime), requestId);
      onSaved();
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };
  return <section className="role-editor" aria-labelledby={titleId}>
    <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); if (!role && step < 2) next(); else void save(); }}>
      <div className="project-aow-dialog-body">
        <div className="project-aow-settings-heading"><div><h2 id={titleId}>{t(role ? 'roles.edit' : 'roles.add')}</h2><p>{t(role ? 'roles.edit_hint' : 'roles.wizard_hint')}</p></div></div>
        {!role && <ol className="role-wizard-steps" aria-label={t('roles.creation_steps')}>
          {['basics', 'runtime', 'connection'].map((key, index) => <li key={key} aria-current={index === step ? 'step' : undefined} className={index < step ? 'complete' : ''}><span>{index < step ? <Check /> : index + 1}</span>{t(`roles.step_${key}`)}</li>)}
        </ol>}
        <div hidden={!role && step !== 0} className="role-step">
        <label className="project-aow-dialog-field"><span>{t('roles.name')}</span><input autoFocus maxLength={120} value={name} disabled={locked} onChange={event => setName(event.target.value)} /></label>
        <label className="project-aow-dialog-field"><span>{t('roles.role_description')}</span><textarea rows={4} maxLength={4096} value={description} disabled={locked} onChange={event => setDescription(event.target.value)} placeholder={t('roles.description_hint')} /></label>
        </div>
        <div hidden={!role && step !== 1} className="role-step">
        <label className="project-aow-dialog-field"><span>{t('settings.runtime')}</span><select aria-label={t('settings.runtime')} value={runtimeKey} disabled={locked || loading} onChange={event => setRuntime(runtimes.find(item => item.key === event.target.value)?.runtime)}>
          <option value="">{t('roles.select_runtime')}</option>
          {runtime && !selectedRuntime && <option value={runtimeKey} disabled>{runtime.mode === 'acp' ? 'ACP' : t('settings.agent_cli')} · {runtime.agent_id} · {t('roles.runtime_unavailable')}</option>}
          {(['acp', 'cli'] as const).map(mode => <optgroup key={mode} label={mode === 'acp' ? 'ACP' : t('settings.agent_cli')}>
            {runtimes.filter(item => item.runtime.mode === mode).map(item => <option key={item.key} value={item.key} disabled={!item.available || !item.preset}>{item.name} · {mode === 'acp' ? 'ACP' : t('settings.agent_cli')} · {item.runtime.agent_id}{item.available && item.preset ? '' : ` · ${t('roles.runtime_unavailable')}`}</option>)}
          </optgroup>)}
        </select></label>
        <p className="project-aow-form-intro">{t('roles.shared_runtime')}</p>
        {!loading && !runtimes.some(item => item.available && item.preset) && <p className="project-aow-empty">{t('roles.runtime_empty')}</p>}
        <button className="project-aow-dialog-button" type="button" disabled={locked || loading} onClick={() => { setError(''); setReload(value => value + 1); }}>{t('roles.reload_runtimes')}</button>
        </div>
        {!role && <div hidden={step !== 2} className="role-step">
          <div className="role-connection-summary"><strong>{name}</strong><span>{runtime?.mode === 'acp' ? 'ACP' : t('settings.agent_cli')} · {selectedRuntime?.name}</span></div>
          <RoleIdentity preset={preset} source={source} onSourceChange={setSource} disabled={locked} />
          <p className="project-aow-form-intro">{t('roles.managed_home')}</p>
          <p className="project-aow-form-intro">{t('roles.test_hint')}</p>
          {busy && <p className="role-connection-progress" role="status"><LoaderCircle className="acp-spinner" />{t('roles.testing')}</p>}
        </div>}
        {role && <div className="role-step">
        <label className="project-aow-dialog-field"><span>{t('roles.instruction')}</span><textarea rows={5} maxLength={131072} value={instruction} disabled={locked} onChange={event => setInstruction(event.target.value)} /></label>
        <label className="project-aow-dialog-field"><span>{t('roles.skills')}</span><textarea rows={3} value={skills} disabled={locked} onChange={event => setSkills(event.target.value)} placeholder={t('roles.skills_hint')} /></label>
          <p className="project-aow-form-intro">{t('roles.managed_home')}</p>
        </div>}
        {error && <p className="project-aow-error" role="alert">{error}{!presets.length && <button className="project-aow-dialog-button" type="button" disabled={locked} onClick={() => { setError(''); setReload(value => value + 1); }}>{t('roles.reload_defaults')}</button>}</p>}
      </div>
      <footer className="project-aow-dialog-footer">
        <button className="project-aow-dialog-button" type="button" disabled={locked} onClick={onClose}>{t('roles.cancel')}</button>
        {!role && step > 0 && <button className="project-aow-dialog-button" type="button" disabled={locked} onClick={() => { setError(''); setStep(value => value - 1); }}><ArrowLeft />{t('roles.previous')}</button>}
        {!role && step < 2 ? <button className="project-aow-dialog-button primary" type="submit" disabled={locked || !name.trim() || (step === 1 && !runtimeReady)}>{t('roles.next')}<ArrowRight /></button>
          : <button className="project-aow-dialog-button primary" type="submit" disabled={locked || !name.trim() || !runtimeReady || (!role && !source.trim())}>{busy && <LoaderCircle className="acp-spinner" />}{t(role ? 'roles.save' : 'roles.test_create')}</button>}
      </footer>
    </form>
  </section>;
}
