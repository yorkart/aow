import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { Plus, Trash2, Upload } from 'lucide-react';
import { prApi } from './api';
import type { ReviewProvider, ReviewProviderSettings as Settings } from './types';
import { MarkdownContent } from '../../components/MarkdownContent';
import protocol from './review-providers.md?raw';
import './ReviewProviderSettings.css';

const ScriptPreview = lazy(() => import('./ReviewScriptPreview'));
const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function ReviewProviderSettings({ active, onBusyChange, onDirtyChange }: {
  active: boolean; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void;
}) {
  const [saved, setSaved] = useState<Settings>();
  const [loadRevision, setLoadRevision] = useState(0);
  const [draft, setDraft] = useState<Settings>();
  const [selected, setSelected] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const upload = useRef<HTMLInputElement>(null);
  const dirty = JSON.stringify(saved) !== JSON.stringify(draft);
  useEffect(() => { onDirtyChange(dirty); }, [dirty, onDirtyChange]);
  useEffect(() => { if (active) onBusyChange(busy); }, [active, busy, onBusyChange]);
  useEffect(() => {
    if (!active || draft) return;
    let live = true;
    setBusy(true);
    void prApi.reviewProviders().then(value => { if (live) { setSaved(value); setDraft(value); } })
      .catch(reason => { if (live) setError(message(reason)); }).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [active, draft, loadRevision]);
  const provider = draft?.providers[selected];
  const update = (patch: Partial<ReviewProvider>) => {
    setDraft(current => current ? { ...current, providers: current.providers.map((p, index) => index === selected ? { ...p, ...patch } : p) } : current);
    setStatus(''); setError('');
  };
  const add = () => {
    if (!draft) return;
    let suffix = draft.providers.length + 1;
    while (draft.providers.some(provider => provider.id === 'provider-' + suffix)) suffix += 1;
    const next: ReviewProvider = { id: 'provider-' + suffix, name: '新 Provider', enabled: true, hosts: ['git.example.com'], script: '' };
    setSelected(draft.providers.length);
    setDraft({ ...draft, providers: [...draft.providers, next] }); setStatus('');
  };
  const save = async () => {
    if (!draft) return;
    setBusy(true); setError(''); setStatus('');
    try { const next = await prApi.saveReviewProviders(draft); setDraft(next); setSaved(next); setStatus('Provider 配置和脚本已保存。'); window.dispatchEvent(new Event('aow-review-providers-changed')); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const validate = async () => {
    if (!provider) return;
    setBusy(true); setError(''); setStatus('');
    try { await prApi.testReviewProvider(provider); setStatus('describe 协议检查通过，已声明 list、detail、diff。保存后可在 Pull Requests 面板验证实际查询。'); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const readFile = async (file?: File) => {
    if (!file) return;
    setBusy(true); setError('');
    try {
      if (file.size > 1024 * 1024) throw new Error('脚本文件最大 1 MiB。');
      update({ script: await file.text() }); setStatus('已读取文件内容，可在下方预览，保存后生效。');
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); if (upload.current) upload.current.value = ''; }
  };
  return <section className="review-provider-settings project-aow-dialog-form" aria-label="Pull Requests 设置">
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>Pull Requests</h2><p>按 Git remote 域名匹配 Provider，由脚本实现约定的 API。</p></div></div>
      <div className="review-provider-toolbar">
        <label>Provider<select aria-label="选择 Provider" disabled={busy || !draft?.providers.length} value={selected} onChange={e => { setSelected(Number(e.target.value)); setStatus(''); }}>
          {draft?.providers.map((p, index) => <option key={index} value={index}>{p.name || p.id}{p.enabled ? '' : '（已停用）'}</option>)}
        </select></label>
        <button className="project-aow-dialog-button" disabled={busy || !draft} onClick={add}><Plus size={14} />添加 Provider</button>
        {provider ? <button className="project-aow-dialog-button" disabled={busy} onClick={() => { setDraft({ ...draft!, providers: draft!.providers.filter((_, i) => i !== selected) }); setSelected(0); setStatus(''); }}><Trash2 size={14} />移除</button> : null}
      </div>
      {provider ? <>
        <div className="review-provider-fields">
          <label className="project-aow-dialog-field"><span>Provider ID</span><input aria-label="Provider ID" value={provider.id} disabled={busy} onChange={e => update({ id: e.target.value })} /></label>
          <label className="project-aow-dialog-field"><span>显示名称</span><input aria-label="Provider 显示名称" value={provider.name} disabled={busy} onChange={e => update({ name: e.target.value })} /></label>
          <label className="project-aow-dialog-field review-provider-hosts"><span>Remote 域名（每行一个）</span><textarea aria-label="Remote 域名" rows={3} spellCheck={false} value={provider.hosts.join('\n')} disabled={busy} onChange={e => update({ hosts: e.target.value.split('\n') })} /></label>
        </div>
        <label className="project-aow-dialog-checkbox"><input type="checkbox" checked={provider.enabled} disabled={busy} onChange={e => update({ enabled: e.target.checked })} />启用此 Provider</label>
        <div className="review-provider-toolbar"><strong>Python 脚本（只读预览）</strong><button className="project-aow-dialog-button" disabled={busy} onClick={() => upload.current?.click()}><Upload size={14} />上传脚本</button>
          <input ref={upload} type="file" aria-label="上传 Provider 脚本" accept=".py,.txt,text/plain,text/x-python" hidden onChange={e => void readFile(e.target.files?.[0])} /></div>
        <p className="project-aow-form-intro">脚本仅支持预览。如需修改，请在本地编辑后上传替换，点击保存后生效。脚本文件由 AoW 管理，运行环境使用 Settings → Environment 中的 python3 和 PATH。</p>
        {active ? <Suspense fallback={<p>正在加载脚本预览…</p>}><ScriptPreview key={provider.id} value={provider.script} /></Suspense> : null}
      </> : draft ? <p>尚未配置 Provider，点击“添加 Provider”开始配置。</p> : busy ? <p>正在加载 Provider…</p> : <button onClick={() => { setError(''); setLoadRevision(value => value + 1); }}>重新加载</button>}
      <details className="review-provider-protocol"><summary>脚本 API 协议与返回字段</summary><MarkdownContent readOnly text={protocol} /></details>
      {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
      {status ? <p role="status">{status}</p> : null}
    </div>
    <footer className="project-aow-dialog-footer">
      {dirty ? <small>有未保存的修改</small> : null}
      <button className="project-aow-dialog-button" disabled={busy || !dirty} onClick={() => { setDraft(saved); setSelected(0); setError(''); setStatus(''); }}>放弃修改</button>
      <button className="project-aow-dialog-button" disabled={busy || !provider} onClick={() => void validate()}>检查协议</button>
      <button className="project-aow-dialog-button primary" disabled={busy || !draft || !dirty} onClick={() => void save()}>{busy ? '处理中…' : '保存'}</button>
    </footer>
  </section>;
}
