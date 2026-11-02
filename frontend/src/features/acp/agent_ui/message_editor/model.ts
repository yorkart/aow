import { t } from '../../../../lib/i18n';
import type { Data } from '../../types';
import { roleMentionText, type RoleReference } from '../../../agent-roles/mentions';

export type EditorPart = { type: 'text'; text: string } | { type: 'image'; id: string } | RoleReference;
export type DraftImage = { status: 'loading' } | { status: 'ready'; data: string; mimeType: string } | { status: 'error'; error: string };
export interface ComposerDraft { parts: EditorPart[]; images: Record<string, DraftImage> }
export type UpdateDraft = (update: ComposerDraft | ((current: ComposerDraft) => ComposerDraft)) => void;
let nextImageId = 0;
export const createImageId = () => `image-${++nextImageId}`;
export const emptyDraft = (): ComposerDraft => ({ parts: [], images: {} });
export const draftText = (draft: ComposerDraft) => draft.parts.map(part => part.type === 'text' ? part.text : '\uFFFC').join('');
export const hasDraftContent = (draft: ComposerDraft) => draft.parts.some(part => part.type !== 'text' || part.text.trim());
export const hasPendingImages = (draft: ComposerDraft) => draft.parts.some(part => part.type === 'image' && draft.images[part.id]?.status !== 'ready');
export const hasImages = (draft: ComposerDraft) => draft.parts.some(part => part.type === 'image');
// The HTTP action endpoint accepts up to 32 MiB, including the JSON envelope.
export const MAX_PROMPT_BYTES = 32 * 1024 * 1024;

export function promptContent(draft: ComposerDraft): Data[] {
  const content: Data[] = [];
  for (const part of draft.parts) {
    if (part.type === 'text' || part.type === 'role') {
      const value = part.type === 'text' ? part.text : roleMentionText(part);
      const last = content.at(-1);
      if (last?.type === 'text') last.text = String(last.text) + value;
      else content.push({ type: 'text', text: value });
      continue;
    }
    const image = draft.images[part.id];
    if (image?.status !== 'ready') throw new Error(t("common.wait_for_images_to_finish_processing"));
    content.push({ type: 'image', data: image.data, mimeType: image.mimeType });
  }
  return content;
}
