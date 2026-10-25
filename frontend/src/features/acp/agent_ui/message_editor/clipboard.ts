import { t } from '../../../../lib/i18n';
import { createImageId, emptyDraft, hasPendingImages, MAX_PROMPT_BYTES, promptContent, type ComposerDraft } from './model';
import { MAX_IMAGE_BYTES } from './image';

// Synchronous clipboard events also work on LAN HTTP. Carry bytes rather than
// editor-local IDs so copied images outlive the source draft and browser tab.
const MIME_TYPE = 'application/x-aow-acp-draft+json';

export function writeClipboard(data: DataTransfer, draft: ComposerDraft) {
  if (hasPendingImages(draft)) throw new Error(t("common.wait_for_images_to_finish_processing_or_remove_failed"));
  const payload = JSON.stringify({ version: 1, content: promptContent(draft) });
  if (new Blob([payload]).size > MAX_PROMPT_BYTES) throw new Error(t("common.selection_exceeds_32_mib_copy_fewer_images_at_a"));
  data.setData(MIME_TYPE, payload);
  data.setData('text/plain', draft.parts.map(part => part.type === 'text' ? part.text : 'Image').join(''));
}

export function readClipboard(data: DataTransfer): ComposerDraft | undefined {
  if (!data.types.includes(MIME_TYPE)) return undefined;
  const payload = data.getData(MIME_TYPE);
  if (new Blob([payload]).size > MAX_PROMPT_BYTES) throw new Error(t("common.clipboard_content_exceeds_32_mib"));
  try {
    const parsed = JSON.parse(payload);
    if (parsed?.version !== 1 || !Array.isArray(parsed.content)) throw new Error();
    const draft = emptyDraft();
    for (const block of parsed.content) {
      if (block?.type === 'text' && typeof block.text === 'string') draft.parts.push({ type: 'text', text: block.text });
      else if (block?.type === 'image' && block.mimeType === 'image/png' && typeof block.data === 'string' && block.data.length > 0 && block.data.length <= 4 * Math.ceil(MAX_IMAGE_BYTES / 3) && /^[A-Za-z0-9+/]+={0,2}$/.test(block.data)) {
        const id = createImageId();
        draft.parts.push({ type: 'image', id });
        draft.images[id] = { status: 'ready', data: block.data, mimeType: block.mimeType };
      } else throw new Error();
    }
    return draft;
  } catch { throw new Error(t("common.invalid_acp_clipboard_content_copy_the_selection_again")); }
}
