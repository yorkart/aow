import { useEffect, useRef } from 'react';
import { t } from '../../lib/i18n';
import { useAowTooltip } from '../AowTooltip';
import type { ConversationTurn } from './ConversationLayout';

function NavigationMark({ prompt, index, active, buttonRef, onNavigate }: {
  prompt: string; index: number; active: boolean;
  buttonRef: (node: HTMLButtonElement | null) => void; onNavigate: () => void;
}) {
  const input = prompt.trim() ? prompt : t('components.empty_input');
  const { triggerProps, closeTooltip, tooltip } = useAowTooltip(<div className="conversation-navigation-preview">{input}</div>, 'right');
  return <li>
    <button type="button" {...triggerProps} ref={buttonRef} aria-label={`${index + 1}. ${input}`}
      aria-current={active ? 'location' : undefined} tabIndex={active ? 0 : -1}
      onClick={() => { closeTooltip(); onNavigate(); }}>
      <span className="conversation-navigation-mark" aria-hidden="true" />
    </button>
    {tooltip}
  </li>;
}

export function ConversationNavigation({ turns, activeTurn, onNavigate }: {
  turns: Pick<ConversationTurn, 'id' | 'prompt'>[]; activeTurn: number; onNavigate: (index: number) => void;
}) {
  const list = useRef<HTMLOListElement>(null);
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);
  useEffect(() => {
    buttons.current.length = turns.length;
    const container = list.current;
    const button = buttons.current[activeTurn];
    if (!container || !button) return;
    // Scroll only the rail: revealing the current mark must not move the transcript.
    const reveal = () => {
      const top = button.getBoundingClientRect().top - container.getBoundingClientRect().top;
      const bottom = top + button.offsetHeight;
      if (top < 4) container.scrollTop += top - 4;
      else if (bottom > container.clientHeight - 4) container.scrollTop += bottom - container.clientHeight + 4;
    };
    reveal();
    const observer = new ResizeObserver(reveal);
    observer.observe(container);
    return () => observer.disconnect();
  }, [activeTurn, turns.length]);

  return <nav className="project-aow-snapshot-navigation" aria-label={t('components.user_input_navigation')}>
    <ol ref={list} onKeyDown={event => {
      if (event.altKey || event.ctrlKey || event.metaKey) return;
      const current = buttons.current.indexOf(event.target as HTMLButtonElement);
      if (current < 0) return;
      const next = event.key === 'ArrowDown' ? Math.min(turns.length - 1, current + 1)
        : event.key === 'ArrowUp' ? Math.max(0, current - 1)
          : event.key === 'Home' ? 0 : event.key === 'End' ? turns.length - 1 : null;
      if (next === null) return;
      event.preventDefault();
      buttons.current[next]?.focus();
    }}>
      {turns.map((turn, index) => <NavigationMark key={turn.id} prompt={turn.prompt} index={index} active={activeTurn === index}
        buttonRef={node => { buttons.current[index] = node; }} onNavigate={() => onNavigate(index)} />)}
    </ol>
  </nav>;
}
