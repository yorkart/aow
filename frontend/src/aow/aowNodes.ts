function nodeUrl(address: string): URL | undefined {
  try {
    const url = new URL(address.trim());
    if (/^https?:\/\//i.test(address.trim()) && url.hostname && !url.username && !url.password) return url;
  } catch { /* Ignore invalid addresses when displaying saved configuration. */ }
}

export function parseNodeAddresses(text: string): string[] {
  const addresses = text.split('\n').flatMap((line, index) => {
    if (!line.trim()) return [];
    const url = nodeUrl(line);
    if (!url) throw new Error(`第 ${index + 1} 行地址无效，请填写完整的 http:// 或 https:// 地址（不含用户名和密码）。`);
    return [url.href];
  });
  return [...new Set(addresses)];
}

export function otherNodeAddresses(addresses: string[], currentAddress: string): string[] {
  const current = new URL(currentAddress);
  const currentPath = current.pathname.replace(/\/+$/, '');
  return [...new Set(addresses.flatMap(address => {
    const url = nodeUrl(address);
    if (!url) return [];
    const path = url.pathname.replace(/\/+$/, '');
    // Accept both the deployment root and a bookmarked desktop/mobile entry.
    const self = url.origin === current.origin
      && (path === currentPath || path === currentPath + '/aow' || path === currentPath + '/m');
    return self ? [] : [url.href];
  }))];
}
