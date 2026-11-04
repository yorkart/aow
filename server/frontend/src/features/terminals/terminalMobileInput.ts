import type { Terminal } from '@xterm/xterm';

// Some soft keyboards emit keydown + input without a usable keyCode or keypress.
// xterm 6 ignores that input while a key is down; recover only text it hasn't sent.
export function installMobileTerminalInput(terminal: Terminal): () => void {
  const host = terminal.element;
  const textarea = terminal.textarea;
  if (!host || !textarea) return () => {};

  let keydown: KeyboardEvent | undefined;
  let keyHandled = false;
  let composing = false;
  let compositionTimer: number | undefined;
  const keyEvent = terminal.onKey(() => { keyHandled = true; });
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.target !== textarea) return;
    keydown = event;
    keyHandled = false;
    // xterm's 229 fallback guesses textarea changes in a timer. A soft keyboard
    // may update it in a later task; use its actual input event instead.
    if (event.keyCode === 229 && !composing) event.stopImmediatePropagation();
  };
  const onKeyUp = () => { keydown = undefined; };
  const onCompositionStart = () => {
    window.clearTimeout(compositionTimer);
    composing = true;
  };
  const onCompositionEnd = () => {
    // xterm commits composition asynchronously after the textarea has updated.
    compositionTimer = window.setTimeout(() => { composing = false; }, 0);
  };
  const onBeforeInput = (event: Event) => {
    if (!(event instanceof InputEvent) || event.target !== textarea || (keydown && keyHandled) || composing || event.isComposing) return;
    const data = event.inputType === 'deleteContentBackward' ? '\x7f'
      : event.inputType === 'insertLineBreak' || event.inputType === 'insertParagraph' ? '\r' : undefined;
    if (!data) return;
    // beforeinput also fires for backspace when the hidden textarea is empty.
    event.preventDefault();
    event.stopImmediatePropagation();
    keyHandled = true;
    terminal.input(data, true);
  };
  const onInput = (event: Event) => {
    if (!(event instanceof InputEvent) || event.target !== textarea || !keydown || keyHandled || composing || event.isComposing
      || event.inputType !== 'insertText' || !event.data) return;
    event.stopImmediatePropagation();
    terminal.input(event.data, true);
  };
  host.addEventListener('keydown', onKeyDown, true);
  host.addEventListener('keyup', onKeyUp, true);
  host.addEventListener('blur', onKeyUp, true);
  host.addEventListener('compositionstart', onCompositionStart, true);
  host.addEventListener('compositionend', onCompositionEnd);
  host.addEventListener('beforeinput', onBeforeInput, true);
  host.addEventListener('input', onInput, true);

  return () => {
    window.clearTimeout(compositionTimer);
    keyEvent.dispose();
    host.removeEventListener('keydown', onKeyDown, true);
    host.removeEventListener('keyup', onKeyUp, true);
    host.removeEventListener('blur', onKeyUp, true);
    host.removeEventListener('compositionstart', onCompositionStart, true);
    host.removeEventListener('compositionend', onCompositionEnd);
    host.removeEventListener('beforeinput', onBeforeInput, true);
    host.removeEventListener('input', onInput, true);
  };
}
