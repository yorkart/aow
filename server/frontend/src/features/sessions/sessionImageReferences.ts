import { filesApi } from '../files/api';

export function sessionImageReferences(text: string): ReadonlyMap<string, string> {
  const images = new Map<string, string>();
  // Some transcripts store the attachment declarations as escaped text.
  const source = text.replace(/&lt;image\b[\s\S]*?&gt;/g, (tag) => {
    const decoder = document.createElement('textarea');
    decoder.innerHTML = tag;
    return decoder.value;
  });
  for (const [tag] of source.matchAll(/<image\b(?:[^<>"']|"[^"]*"|'[^']*')*>/g)) {
    const name = tag.match(/\bname\s*=\s*(?:"(\[Image #\d+\])"|'(\[Image #\d+\])'|(\[Image #\d+\]))/);
    const path = tag.match(/\bpath\s*=\s*(?:"([^"]+)"|'([^']+)')/);
    const label = name?.[1] ?? name?.[2] ?? name?.[3];
    const file = path?.[1] ?? path?.[2];
    if (label && file?.startsWith('/') && !file.includes('\0') && /\.(png|jpe?g|gif|webp|svg|avif|ico|bmp)$/i.test(file)) {
      images.set(label, filesApi.rawUrl(file));
    }
  }
  return images;
}
