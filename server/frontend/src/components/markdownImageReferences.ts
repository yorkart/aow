export function linkImageReferences(content: HTMLElement, images: ReadonlyMap<string, string>) {
  const walker = document.createTreeWalker(content, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => node.parentElement?.closest('a, pre, code, button, script, style, textarea')
      ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  const nodes: Text[] = [];
  while (walker.nextNode()) nodes.push(walker.currentNode as Text);
  for (const node of nodes) {
    const text = node.data;
    const fragment = document.createDocumentFragment();
    let offset = 0;
    // Consume declarations whole so their names and file paths stay unchanged.
    for (const match of text.matchAll(/<image\b[^>]*>|\[Image #\d+\]/g)) {
      const url = images.get(match[0]);
      if (!url) continue;
      fragment.append(text.slice(offset, match.index));
      const link = document.createElement('a');
      link.className = 'session-image-reference';
      link.dataset.sessionImage = match[0];
      link.textContent = match[0];
      link.href = url;
      link.target = '_blank';
      link.rel = 'noopener noreferrer';
      fragment.append(link);
      offset = match.index + match[0].length;
    }
    if (offset) {
      fragment.append(text.slice(offset));
      node.replaceWith(fragment);
    }
  }
}
