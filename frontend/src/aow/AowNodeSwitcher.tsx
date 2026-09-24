import { appUrl, appBaseUrl } from '../lib/basePath';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown, ExternalLink } from 'lucide-react';
import { otherNodeAddresses } from './aowNodes';
import './aow-node-switcher.css';

function NodeMenu({ anchor, addresses, mobile, error, onClose }: {
  anchor: HTMLButtonElement;
  addresses?: string[];
  mobile: boolean;
  error?: string;
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  const nodes = otherNodeAddresses(addresses ?? [], appBaseUrl());
  useLayoutEffect(() => {
    const trigger = anchor.getBoundingClientRect();
    const bounds = menu.current!.getBoundingClientRect();
    setPosition({
      x: Math.max(4, Math.min(trigger.left, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(trigger.bottom, window.innerHeight - bounds.height - 4)),
    });
    (menu.current?.querySelector<HTMLAnchorElement>('a') ?? menu.current)?.focus();
  }, [anchor, addresses]);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node) && !anchor.contains(event.target as Node)) onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape' || event.key === 'Tab') {
        if (event.key === 'Escape') event.preventDefault();
        onClose();
        anchor.focus();
      }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const links = [...menu.current!.querySelectorAll<HTMLAnchorElement>('a')];
      if (!links.length) return;
      const current = links.indexOf(document.activeElement as HTMLAnchorElement);
      const index = event.key === 'Home' ? 0 : event.key === 'End' ? links.length - 1
        : (current + (event.key === 'ArrowDown' ? 1 : -1) + links.length) % links.length;
      links[index].focus();
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('resize', onClose);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('keydown', key);
    };
  }, [anchor, onClose]);
  return createPortal(<div ref={menu} id="aow-node-menu" className={`project-aow-context-menu aow-node-menu${mobile ? ' mobile-aow-node-menu' : ''}`}
    style={{ left: position.x, top: position.y }} role="menu" aria-label="其他 AOW 节点" tabIndex={-1}>
    {nodes.map(address => <a key={address} href={address} target="_blank" rel="noopener noreferrer" role="menuitem" title={address} onClick={onClose}>
      <span>{address}</span><ExternalLink aria-hidden="true" />
    </a>)}
    {error ? <p className="aow-node-empty" role="alert">节点地址加载失败：{error}</p>
      : !nodes.length ? <p className="aow-node-empty">{addresses ? '暂无其他节点，可在 Settings → Nodes 中配置。' : '节点地址加载中…'}</p> : null}
  </div>, document.body);
}

export function AowNodeSwitcher({ addresses, mobile = false, error }: { addresses?: string[]; mobile?: boolean; error?: string }) {
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  return <>
    <button ref={trigger} type="button" className="aow-node-trigger" title="切换 AOW 节点"
      aria-label="切换 AOW 节点" aria-haspopup="menu" aria-expanded={open} aria-controls={open ? 'aow-node-menu' : undefined}
      onClick={() => setOpen(value => !value)} onKeyDown={event => {
        if (event.key === 'ArrowDown' && !open) { event.preventDefault(); setOpen(true); }
      }}>
      <img src={appUrl('/workspace-icon.svg')} width={16} height={16} alt="" aria-hidden="true" draggable={false} />
      <strong>AoW</strong>{mobile ? <span className="mobile-node-subtitle">移动工作台</span> : null}<ChevronDown aria-hidden="true" />
    </button>
    {open && trigger.current ? <NodeMenu anchor={trigger.current} addresses={addresses} mobile={mobile} error={error} onClose={close} /> : null}
  </>;
}
