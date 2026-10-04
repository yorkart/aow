import { useEffect, useRef, type RefObject } from 'react';

export function useEditorDismiss(root: RefObject<HTMLElement | null>, active: boolean, finish: (focus?: boolean) => void) {
  const callback = useRef(finish); callback.current = finish;
  useEffect(() => {
    if (!active) return;
    const inside = (target: Element) => {
      if (root.current?.contains(target)) return true;
      // Project/tag menus are portalled, but still belong to the edited row.
      const owner = target.closest('[data-inbox-editor-owner]')?.getAttribute('data-inbox-editor-owner');
      return !!owner && owner === root.current?.getAttribute('data-inbox-editor');
    };
    const outside = (event: Event) => {
      if (event.target instanceof Element && !inside(event.target)) callback.current();
    };
    const keyboard = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || !(event.target instanceof Element) || !inside(event.target)) return;
      if (event.key === 'Escape' && event.target.closest('[data-inbox-editor-owner]')) return;
      if ((event.key === 'Enter' && (event.metaKey || event.ctrlKey)) || event.key === 'Escape') {
        event.preventDefault(); callback.current(true);
      }
    };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('focusin', outside);
    document.addEventListener('keydown', keyboard);
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('focusin', outside); document.removeEventListener('keydown', keyboard); };
  }, [root, active]);
}
