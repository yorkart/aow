const imageExtensions: Record<string, string> = {
  'image/png': 'png', 'image/jpeg': 'jpg', 'image/gif': 'gif', 'image/webp': 'webp',
  'image/bmp': 'bmp', 'image/tiff': 'tiff', 'image/avif': 'avif', 'image/svg+xml': 'svg',
};

function screenshotFile(blob: Blob): File {
  const extension = imageExtensions[blob.type.toLowerCase()];
  if (!extension) throw new Error(`暂不支持此剪贴板图片格式：${blob.type}`);
  const stamp = new Date().toISOString().replace(/[-:]/g, '').replace('T', '-').replace(/\.\d+Z$/, '');
  return new File([blob], `screenshot-${stamp}.${extension}`, { type: blob.type });
}

export function pastedFiles(data: DataTransfer): { files: File[]; hasDirectories: boolean } {
  const items = Array.from(data.items);
  const hasDirectories = items.some((item) => item.kind === 'file' && item.webkitGetAsEntry?.()?.isDirectory);
  const files = items.length
    ? items.filter((item) => item.kind === 'file' && !item.webkitGetAsEntry?.()?.isDirectory)
      .map((item) => item.getAsFile()).filter((file): file is File => file !== null)
    : Array.from(data.files);
  return {
    // Browsers generally name a screenshot "image.png". Preserve all other
    // file names, including text files (which differ from copied plain text).
    files: files.map((file) => imageExtensions[file.type.toLowerCase()] && (!file.name || /^image\.(png|jpe?g|gif|webp)$/i.test(file.name))
      ? screenshotFile(file) : file),
    hasDirectories,
  };
}

export async function readClipboardImages(): Promise<File[]> {
  // Call read immediately from the click handler, before other asynchronous
  // work can consume the browser's transient user activation.
  const items = await navigator.clipboard.read();
  const files: File[] = [];
  for (const item of items) {
    // One clipboard item can expose several representations of one image.
    const type = item.types.find((type) => type === 'image/png')
      ?? item.types.find((type) => imageExtensions[type.toLowerCase()]);
    if (type) files.push(screenshotFile(await item.getType(type)));
  }
  return files;
}

export function isEditableTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && Boolean(target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])'));
}
