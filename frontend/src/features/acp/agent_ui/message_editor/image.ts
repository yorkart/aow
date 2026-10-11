// Browser adaptation of LanguageModelImage::from_image. ACP carries the PNG bytes,
// not a browser object URL or a server-side attachment path.
const MAX_DIMENSION = 1568;
export const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const MAX_DOWNSCALE_PASSES = 8;

export async function prepareImage(file: File): Promise<{ data: string; mimeType: string }> {
  if (!file.size || !/^image\/(png|jpe?g|webp|gif|bmp|tiff|avif|x-icon|vnd.microsoft.icon)$/i.test(file.type)) throw new Error('Unsupported or empty image.');
  const bitmap = await createImageBitmap(file).catch(() => { throw new Error('Failed to decode image.'); });
  try {
    const scale = Math.min(1, MAX_DIMENSION / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Image processing is unavailable.');
    let blob: Blob | null = null;
    for (let pass = 0; pass <= MAX_DOWNSCALE_PASSES; pass++) {
      context.imageSmoothingQuality = 'high';
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      blob = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, 'image/png'));
      if (!blob) throw new Error('Failed to encode image.');
      if (blob.size <= MAX_IMAGE_BYTES) break;
      canvas.width = Math.max(1, Math.round(canvas.width * .85));
      canvas.height = Math.max(1, Math.round(canvas.height * .85));
    }
    if (!blob || blob.size > MAX_IMAGE_BYTES) throw new Error('Image exceeds 5 MiB after resizing.');
    const data = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(',')[1]);
      reader.onerror = () => reject(new Error('Failed to read image.'));
      reader.readAsDataURL(blob);
    });
    return { data, mimeType: 'image/png' };
  } finally { bitmap.close(); }
}
