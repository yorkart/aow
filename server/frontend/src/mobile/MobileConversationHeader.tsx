import { useId, useRef, useState, type ReactNode } from 'react';
import { Check, Ellipsis } from 'lucide-react';
import { ConversationIds } from '../components/conversation/ConversationIds';
import { RowMenu } from '../components/RowMenu';
import { t } from '../lib/i18n';
import { MobilePageHeader } from './MobilePrimitives';
import './mobile-conversation-header.css';

export interface MobileConversationAction {
  label: string; icon: ReactNode; onClick: () => void; disabled?: boolean; checked?: boolean;
}

export function MobileConversationHeader({ title, subtitle, back, backLabel, identifiers, actions = [] }: {
  title: string; subtitle?: string; back: () => void; backLabel?: string;
  identifiers: { label: string; value?: string | null }[]; actions?: MobileConversationAction[];
}) {
  const id = useId();
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return <MobilePageHeader title={title} subtitle={subtitle} back={back} backLabel={backLabel} actions={<>
    <ConversationIds identifiers={identifiers} mobile label={t("mobile.session_information")} />
    <button ref={button} type="button" className="mobile-icon-button" aria-label={t("mobile.more_actions")} aria-haspopup="menu"
      aria-expanded={open} aria-controls={open ? id : undefined} disabled={!actions.length} onClick={() => setOpen(value => !value)}><Ellipsis size={21} /></button>
    {open && <RowMenu id={id} label={t("mobile.more_actions")} anchor={button} onClose={() => setOpen(false)} className="mobile-conversation-menu">
      {actions.map(action => <button key={action.label} type="button" role={action.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
        aria-checked={action.checked} disabled={action.disabled} onClick={() => {
          setOpen(false); button.current?.focus({ preventScroll: true }); action.onClick();
        }}>{action.icon}<span>{action.label}</span>{action.checked && <Check size={17} />}</button>)}
    </RowMenu>}
  </>} />;
}
