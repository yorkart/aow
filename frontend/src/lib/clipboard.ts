export async function copyText(value: string) {
  try {
    if (navigator.clipboard) { await navigator.clipboard.writeText(value); return; }
  } catch { /* Insecure origins and denied clipboard permissions use the fallback. */ }
  const focused = document.activeElement;
  const selection = window.getSelection();
  const ranges = Array.from({ length: selection?.rangeCount ?? 0 }, (_, index) => selection!.getRangeAt(index).cloneRange());
  const input = document.createElement('textarea');
  input.value = value; input.readOnly = true; input.className = 'acp-clipboard-input';
  Object.assign(input.style, { position: 'fixed', top: '0', left: '-9999px', opacity: '0' });
  document.body.append(input);
  try {
    input.select();
    if (!document.execCommand('copy')) throw new Error('copy failed');
  } finally {
    input.remove();
    if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
    selection?.removeAllRanges(); ranges.forEach(range => selection?.addRange(range));
  }
}
