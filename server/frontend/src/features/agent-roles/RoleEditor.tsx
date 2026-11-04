import { useEffect, useId, useState } from 'react';
import { ArrowLeft, ArrowRight, Check, LoaderCircle } from 'lucide-react';
import { t } from '../../lib/i18n';
import { RoleAcpRuntime } from './RoleAcpRuntime';
import { RoleIdentity } from './RoleIdentity';
import { agentsApi } from '../agents/api';
import type { AowAgent } from '../agents/types';
import { rolesApi, type AgentRole, type RoleConfig, type IdentitySource } from './api';

export function RoleEditor({ role, onClose, onSaved, onBusyChange, onDirtyChange }: { role?: AgentRole; onClose: () => void; onSaved: () => void; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void }) {
  const [requestId] = useState(() => Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join(''));
  const [step, setStep] = useState(0);
  const [description, setDescription] = useState(role?.description ?? '');
  const [name, setName] = useState(role?.name ?? '');
  const [mode, setMode] = useState<'cli' | 'acp'>(role?.runtime.mode ?? 'acp');
  const [agentId, setAgentId] = useState(role?.runtime.mode === 'acp' ? role.runtime.agent_id : '');
  const [cliId, setCliId] = useState(role?.runtime.mode === 'cli' ? role.runtime.agent_id : '');
  const [source, setSource] = useState(role?.identity.source ?? '');
  const [presets, setPresets] = useState<IdentitySource[]>([]);
  const [reload, setReload] = useState(0);
  const [instruction, setInstruction] = useState(role?.instruction ?? '');
  const [skills, setSkills] = useState(role?.skills.join('\n') ?? '');
  const [localAgents, setLocalAgents] = useState<AowAgent[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [acpReady, setAcpReady] = useState(false);
  const [loading, setLoading] = useState(true);
  const titleId = useId();
  const locked = busy;
  const configuredCli = localAgents.filter(agent => agent.source === 'configured' && presets.some(preset => preset.layout === agent.agent_type));
  const selectedCli = configuredCli.find(agent => agent.id === cliId);
  const preset = presets.find(item => mode === 'cli' ? item.layout === selectedCli?.agent_type : agentId.toLowerCase().split(/[^a-z0-9]+/).includes(item.layout));
  const runtimeReady = !loading && !!preset && (mode === 'acp' ? acpReady : !!selectedCli?.available);
  const draft = JSON.stringify([name, description, mode, agentId, cliId, source, instruction, skills]);
  const [baseline] = useState(draft);
  const dirty = draft !== baseline;
  useEffect(() => { onBusyChange(busy); return () => onBusyChange(false); }, [busy, onBusyChange]);
  useEffect(() => { onDirtyChange(dirty); return () => onDirtyChange(false); }, [dirty, onDirtyChange]);
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void Promise.all([agentsApi.agents(true), rolesApi.sources()]).then(([agents, sources]) => {
      if (!cancelled) { setLocalAgents(agents); setPresets(sources); }
    }).catch(reason => { if (!cancelled) { setLocalAgents([]); setPresets([]); setError(String(reason)); } }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [reload]);
  const config = (): RoleConfig => ({
    name: name.trim(), description: description.trim(), runtime: { mode, agent_id: mode === 'acp' ? agentId : cliId },
    ...(role ? { home_layout: role.home_layout } : {}), identity: role?.identity ?? { kind: 'local_home', source: source.trim() }, instruction, skills: skills.split('\n').map(value => value.trim()).filter(Boolean),
  });
  const next = () => {
    if (locked || !name.trim() || (step === 1 && !runtimeReady)) return;
    setError('');
    setStep(current => current + 1);
  };
  const save = async () => {
    if (locked || !name.trim() || !runtimeReady || (!role && !source.trim())) return;
    setBusy(true); setError('');
    try {
      if (role) await rolesApi.save(config(), role.id);
      else await rolesApi.testAndCreate(config(), requestId);
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
        <label className="project-aow-dialog-field"><span>{t('roles.mode')}</span><select aria-label={t('roles.mode')} value={mode} disabled={locked} onChange={event => setMode(event.target.value as 'cli' | 'acp')}><option value="acp">ACP</option><option value="cli">CLI</option></select></label>
        {mode === 'acp' ? <RoleAcpRuntime layouts={presets.map(item => item.layout)} value={agentId} onChange={setAgentId} disabled={busy} onReadyChange={setAcpReady} /> : <>
          <label className="project-aow-dialog-field"><span>{t('roles.local_runtime')}</span><select aria-label={t('roles.local_runtime')} value={cliId} disabled={locked || loading} onChange={event => setCliId(event.target.value)}><option value="">{t('roles.select_local_runtime')}</option>{cliId && !selectedCli && <option value={cliId} disabled>{cliId} · {t('roles.runtime_unavailable')}</option>}{configuredCli.map(agent => <option key={agent.id} value={agent.id} disabled={!agent.available}>{agent.display_name}{agent.available ? '' : ` · ${t('roles.runtime_unavailable')}`}</option>)}</select></label>
          <p className="project-aow-form-intro">{t('roles.shared_cli')}</p>
          {!loading && !configuredCli.some(agent => agent.available) && <p className="project-aow-empty">{t('roles.cli_empty')}</p>}
          <button className="project-aow-dialog-button" type="button" disabled={locked || loading} onClick={() => { setError(''); setReload(value => value + 1); }}>{t('roles.reload_runtimes')}</button>
        </>}
        </div>
        {!role && <div hidden={step !== 2} className="role-step">
          <div className="role-connection-summary"><strong>{name}</strong><span>{mode.toUpperCase()} · {mode === 'acp' ? agentId : selectedCli?.display_name}</span></div>
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
