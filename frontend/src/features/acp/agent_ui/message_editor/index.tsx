import { t } from '../../../../lib/i18n';
import { useLayoutEffect, useRef, useState, type ClipboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { text, failure, type Data } from '../../types';
import { createImageId, draftText, hasImages, type ComposerDraft, type UpdateDraft } from './model';
import { prepareImage } from './image';
import { imageChip, insertAtSelection, partsHTML, readParts, selectedParts, selectEnd, writeParts } from './dom';
import { readClipboard, writeClipboard } from './clipboard';
import './message_editor.css';

export function MessageEditor({ draft, onChange, commands: advertised, supportsImages, disabled, placeholder, onSubmit }: {
  draft: ComposerDraft; onChange: UpdateDraft; commands: Data[]; supportsImages: boolean; disabled: boolean; placeholder: string; onSubmit: () => void;
}) {
  const editor = useRef<HTMLDivElement>(null);
  const [commandIndex, setCommandIndex] = useState(0);
  const [error, setError] = useState('');
  const [preview, setPreview] = useState<{ id: string; rect: DOMRect }>();
  const prompt = draftText(draft);
  const commands = /^\/[^\s]*$/.test(prompt) ? advertised.filter(command => text(command.name).toLocaleLowerCase().includes(prompt.slice(1).toLocaleLowerCase())) : [];
  useLayoutEffect(() => {
    const element = editor.current;
    if (!element) return;
    if (JSON.stringify(readParts(element)) !== JSON.stringify(draft.parts)) writeParts(element, draft.parts);
    for (const chip of element.querySelectorAll<HTMLElement>('[data-image-id]')) {
      const image = draft.images[chip.dataset.imageId!];
      chip.dataset.status = image?.status ?? 'error';
      chip.setAttribute('aria-busy', String(image?.status === 'loading'));
      chip.title = image?.status === 'error' ? image.error : '';
    }
  }, [draft]);
  const edited = () => {
    if (!editor.current) return;
    const parts = readParts(editor.current).filter(part => part.type !== 'image' || draft.images[part.id]);
    onChange(current => JSON.stringify(current.parts) === JSON.stringify(parts) ? current : { ...current, parts });
    setCommandIndex(0);
  };
  const chooseCommand = (name: string) => {
    const element = editor.current;
    if (!element) return;
    element.focus();
    const range = document.createRange(); range.selectNodeContents(element);
    const selection = window.getSelection(); selection?.removeAllRanges(); selection?.addRange(range);
    insertAtSelection(element, 'insertText', `/${name} `); edited(); selectEnd(element);
  };
  const pasteImages = (files: File[]) => {
    const element = editor.current;
    if (!element) return;
    if (!supportsImages) { setError('This agent does not support images.'); return; }
    setError('');
    const images = files.map(file => ({ file, id: createImageId() }));
    // Insert placeholders synchronously at the paste caret before conversion starts.
    insertAtSelection(element, 'insertHTML', images.map(image => `${imageChip(image.id).outerHTML} `).join(''));
    const parts = readParts(element);
    onChange(current => ({ ...current, parts, images: { ...current.images, ...Object.fromEntries(images.map(image => [image.id, { status: 'loading' }])) } }));
    for (const { file, id } of images) void prepareImage(file).then(
      image => onChange(current => current.images[id] ? { ...current, images: { ...current.images, [id]: { status: 'ready', ...image } } } : current),
      reason => onChange(current => current.images[id] ? { ...current, images: { ...current.images, [id]: { status: 'error', error: failure(reason) } } } : current),
    );
  };
  const copySelection = (event: ClipboardEvent<HTMLDivElement>, cut = false) => {
    if (cut && disabled) return;
    const selection = { parts: selectedParts(event.currentTarget), images: draft.images };
    if (!hasImages(selection)) return;
    event.preventDefault();
    try {
      writeClipboard(event.clipboardData, selection);
      setError('');
      if (cut) { insertAtSelection(event.currentTarget, 'delete'); edited(); }
    } catch (reason) { setError(failure(reason)); }
  };
  const showPreview = (target: EventTarget | null) => {
    const chip = target instanceof Element ? target.closest<HTMLElement>('[data-image-id]') : null;
    if (chip && editor.current?.contains(chip)) setPreview({ id: chip.dataset.imageId!, rect: chip.getBoundingClientRect() });
    else setPreview(undefined);
  };
  const selectedImage = preview && draft.parts.some(part => part.type === 'image' && part.id === preview.id) ? draft.images[preview.id] : undefined;
  const failedImages = draft.parts.flatMap(part => {
    const image = part.type === 'image' ? draft.images[part.id] : undefined;
    return image?.status === 'error' ? [image.error] : [];
  });
  return <>
    {!!commands.length && !disabled && <div className="acp-command-menu" role="listbox" aria-label={t("acp.agent_command")}>{commands.map((command, index) => <button type="button" key={text(command.name)} role="option" aria-selected={index === commandIndex % commands.length} onMouseDown={event => event.preventDefault()} onClick={() => chooseCommand(text(command.name))}><strong>{text(command.name)}</strong><span>{text(command.description)}</span></button>)}</div>}
    <div ref={editor} className="acp-message-editor" role="textbox" aria-label={t("acp.acp_messages")} aria-multiline="true" aria-disabled={disabled} data-placeholder={placeholder} contentEditable={!disabled} suppressContentEditableWarning
      onInput={edited}
      onCopy={event => copySelection(event)} onCut={event => copySelection(event, true)}
      onPaste={event => {
        if (disabled) return;
        event.preventDefault();
        try {
          const copied = readClipboard(event.clipboardData);
          if (copied) {
            if (hasImages(copied) && !supportsImages) { setError('This agent does not support images.'); return; }
            setError('');
            insertAtSelection(event.currentTarget, 'insertHTML', partsHTML(copied.parts));
            const parts = readParts(event.currentTarget);
            onChange(current => ({ ...current, parts, images: { ...current.images, ...copied.images } }));
            return;
          }
        } catch (reason) { setError(failure(reason)); return; }
        const files = Array.from(event.clipboardData.items).filter(item => item.kind === 'file' && item.type.startsWith('image/')).map(item => item.getAsFile()).filter((file): file is File => !!file);
        if (files.length) pasteImages(files);
        else { insertAtSelection(event.currentTarget, 'insertText', event.clipboardData.getData('text/plain')); edited(); }
      }}
      onDrop={event => event.preventDefault()}
      onMouseOver={event => showPreview(event.target)} onMouseLeave={() => setPreview(undefined)} onFocus={event => showPreview(event.target)} onBlur={() => setPreview(undefined)}
      onKeyDown={event => {
        if (disabled || event.nativeEvent.isComposing) return;
        setError(''); setPreview(undefined);
        const chip = event.target instanceof HTMLElement ? event.target.closest('[data-image-id]') : null;
        if (chip && ['Backspace', 'Delete'].includes(event.key)) {
          event.preventDefault();
          event.currentTarget.focus();
          const range = document.createRange(); range.selectNode(chip);
          const selection = window.getSelection(); selection?.removeAllRanges(); selection?.addRange(range);
          insertAtSelection(event.currentTarget, 'delete'); edited();
        } else if (commands.length && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
          event.preventDefault(); setCommandIndex(index => (index + (event.key === 'ArrowUp' ? -1 : 1) + commands.length) % commands.length);
        } else if (commands.length && ['Enter', 'Tab'].includes(event.key) && !event.shiftKey) {
          event.preventDefault(); chooseCommand(text(commands[commandIndex % commands.length].name));
        } else if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); onSubmit(); }
      }} />
    {(error || failedImages.length > 0) && <p className="acp-error acp-image-error" role="alert">{error || failedImages.join(' ')}</p>}
    {preview && selectedImage?.status === 'ready' && createPortal(<div className="acp-image-preview" role="tooltip" style={{ left: Math.max(8, Math.min(preview.rect.left, window.innerWidth - 328)), ...(preview.rect.top > Math.min(300, window.innerHeight * .4 + 20) ? { bottom: window.innerHeight - preview.rect.top + 6 } : { top: preview.rect.bottom + 6 }) }}><img alt={t("navigation.image_preview")} src={`data:${selectedImage.mimeType};base64,${selectedImage.data}`} /></div>, document.body)}
  </>;
}
