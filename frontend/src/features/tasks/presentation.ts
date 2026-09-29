export function parseInboxMarkdown(markdown: string) {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const index = lines.findIndex(line => line.trim().length > 0);
  const description = lines.slice(index + 1).join('\n');
  return index < 0 ? { title: '', description: '', titleLine: 0 }
    : { title: lines[index].trim(), description: description.trim() ? description : '', titleLine: index + 1 };
}

export function inboxAge(createdAt: string, now: number) {
  const timestamp = Date.parse(createdAt);
  if (!Number.isFinite(timestamp)) return '时间未知';
  const minutes = Math.max(0, Math.floor((now - timestamp) / 60_000));
  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时${minutes % 60 ? ` ${minutes % 60} 分钟` : ''}前`;
  return `${Math.floor(hours / 24)} 天${hours % 24 ? ` ${hours % 24} 小时` : ''}前`;
}
