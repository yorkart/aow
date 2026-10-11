import type { EditorPart } from './model';

const isBlock = (node: Node) => node instanceof HTMLElement && ['DIV', 'P'].includes(node.tagName);

// Native contenteditable uses both text newlines and DIV/BR line boundaries.
// A final BR is the browser's empty-line caret placeholder, not another newline.
export function readParts(root: Node): EditorPart[] {
  const parts: EditorPart[] = [];
  const append = (value: string) => {
    if (!value) return;
    const last = parts.at(-1);
    if (last?.type === 'text') last.text += value;
    else parts.push({ type: 'text', text: value });
  };
  const walk = (parent: Node) => Array.from(parent.childNodes).forEach((node, index, siblings) => {
    if (node.nodeType === Node.TEXT_NODE) { append(node.textContent ?? ''); return; }
    if (!(node instanceof HTMLElement)) return;
    if (node.dataset.imageId) { parts.push({ type: 'image', id: node.dataset.imageId }); return; }
    if (node.tagName === 'BR') { if (index < siblings.length - 1) append('\n'); return; }
    if (isBlock(node) && index > 0) append('\n');
    walk(node);
    if (isBlock(node) && siblings[index + 1] && !isBlock(siblings[index + 1])) append('\n');
  });
  walk(root);
  return parts;
}

export function imageChip(id: string): HTMLSpanElement {
  const chip = document.createElement('span');
  chip.dataset.imageId = id;
  chip.className = 'acp-image-mention';
  chip.contentEditable = 'false';
  chip.setAttribute('role', 'button');
  chip.setAttribute('aria-label', 'Image');
  chip.tabIndex = 0;
  const icon = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  icon.setAttribute('viewBox', '0 0 24 24');
  icon.setAttribute('aria-hidden', 'true');
  for (const d of ['M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z', 'm3 16 5-5 4 4 4-6 5 7', 'M8 7h.01']) {
    const path = document.createElementNS(icon.namespaceURI, 'path'); path.setAttribute('d', d); icon.append(path);
  }
  chip.append(icon, document.createTextNode('Image'));
  return chip;
}

export function writeParts(editor: HTMLElement, parts: EditorPart[]) {
  editor.replaceChildren(...parts.map(part => part.type === 'text' ? document.createTextNode(part.text) : imageChip(part.id)));
}

export function selectedParts(editor: HTMLElement): EditorPart[] {
  const selection = window.getSelection();
  if (!selection?.rangeCount || selection.isCollapsed || !editor.contains(selection.anchorNode) || !editor.contains(selection.focusNode)) return [];
  return readParts(selection.getRangeAt(0).cloneContents());
}

export function partsHTML(parts: EditorPart[]): string {
  const container = document.createElement('div');
  writeParts(container, parts);
  return container.innerHTML;
}

export function selectEnd(editor: HTMLElement) {
  editor.focus();
  const range = document.createRange(); range.selectNodeContents(editor); range.collapse(false);
  const selection = window.getSelection(); selection?.removeAllRanges(); selection?.addRange(range);
}

// execCommand is retained here because Range.insertNode bypasses native undo.
// Only locally constructed chips and escaped text are inserted, never clipboard HTML.
export function insertAtSelection(editor: HTMLElement, command: 'insertText' | 'insertHTML' | 'delete', value?: string) {
  const selection = window.getSelection();
  if (!selection?.rangeCount || !editor.contains(selection.anchorNode) || !editor.contains(selection.focusNode)) selectEnd(editor);
  const moveCaretOutsideImage = () => {
    const anchor = selection?.anchorNode;
    const chip = (anchor instanceof Element ? anchor : anchor?.parentElement)?.closest('[data-image-id]');
    if (!selection?.isCollapsed || !chip || !editor.contains(chip)) return;
    const range = document.createRange(); range.setStartAfter(chip); range.collapse(true);
    selection.removeAllRanges(); selection.addRange(range);
  };
  // Chromium can leave the caret inside the non-editable chip after insertHTML
  // or redo, which prevents the next image/text insertion.
  moveCaretOutsideImage();
  editor.focus();
  document.execCommand(command, false, value);
  moveCaretOutsideImage();
}
