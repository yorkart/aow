import { useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, Copy, LoaderCircle, Share2, X } from 'lucide-react';
import { sessionShareApi, type SessionShare } from './sessionShareApi';
import type { AowAgentSession } from './types';
import './session-share.css';

interface Props { session: AowAgentSession; workspacePath: string }

export function SessionShareButton(props: Props) {
  const [open, setOpen] = useState(false);
  return <>
    <button type="button" className="session-share-button" title="分享会话" aria-label="分享会话" onClick={() => setOpen(true)}><Share2 size={16} /><span>分享</span></button>
    {open && <ShareDialog key={`${props.session.id}:${props.workspacePath}`} {...props} close={() => setOpen(false)} />}
  </>;
}

function ShareDialog({ session, workspacePath, close }: Props & { close: () => void }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const linkRef = useRef<HTMLInputElement>(null);
  const busyRef = useRef(false);
  const [share, setShare] = useState<SessionShare | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const id = useId();
  const url = share ? new URL(share.path, window.location.origin).href : '';

  useEffect(() => {
    const dialog = dialogRef.current!;
    dialog.showModal();
    const controller = new AbortController();
    void sessionShareApi.info(session, controller.signal).then(setShare).catch(reason => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : '无法读取分享设置');
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => { controller.abort(); dialog.close(); };
  }, [session.agent, session.session_id]);

  const copy = async (value: string) => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error('clipboard unavailable');
      await navigator.clipboard.writeText(value);
      setNotice('链接已复制');
    } catch {
      // HTTP deployments may not expose Clipboard API. Keep the link selectable.
      const input = linkRef.current;
      if (input) { input.value = value; input.focus(); input.select(); }
      try {
        if (!input || !document.execCommand('copy')) throw new Error('copy failed');
        setNotice('链接已复制');
      } catch { setNotice('链接已生成，请选中链接手动复制'); }
    }
  };

  const mutate = async (action: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true); setError(''); setNotice('');
    try { await action(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : '操作失败，请重试'); }
    finally { busyRef.current = false; setBusy(false); }
  };

  const createAndCopy = () => mutate(async () => {
    const result = share ?? await sessionShareApi.create(session, workspacePath);
    setShare(result);
    await copy(new URL(result.path, window.location.origin).href);
  });
  const revoke = () => mutate(async () => {
    if (!share) return;
    await sessionShareApi.revoke(share.id);
    setShare(null);
    setNotice('已取消分享，旧链接已失效');
  });

  return createPortal(<dialog ref={dialogRef} className="session-share-dialog" aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`}
    onCancel={event => { event.preventDefault(); if (!busyRef.current) close(); }} onKeyDown={event => event.stopPropagation()}>
    <header><h2 id={`${id}-title`}>分享会话</h2><button type="button" aria-label="关闭分享窗口" disabled={busy} onClick={close}><X size={18} /></button></header>
    <p className="session-share-title">{session.title}</p>
    <p id={`${id}-description`}>持有链接的人无需登录，即可只读查看此会话及后续对话，包括处理过程和工具详情。可以随时取消分享。</p>
    <label htmlFor={`${id}-link`}>分享链接</label>
    <input ref={linkRef} id={`${id}-link`} value={url} readOnly placeholder={loading ? '正在读取分享设置…' : '创建后显示分享链接'} onFocus={event => event.currentTarget.select()} />
    {error && <p className="session-share-error" role="alert">{error}</p>}
    {notice && <p className="session-share-notice" role="status">{notice === '链接已复制' && <Check size={14} />}{notice}</p>}
    <footer>{share && <button type="button" className="danger" disabled={busy || loading} onClick={() => void revoke()}>取消分享</button>}
      <button type="button" className="primary" disabled={busy || loading} onClick={() => void createAndCopy()}>
        {busy || loading ? <LoaderCircle size={15} className="spinning" /> : <Copy size={15} />}{share ? '复制链接' : '创建并复制链接'}
      </button></footer>
  </dialog>, document.body);
}
