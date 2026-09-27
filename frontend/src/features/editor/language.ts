const languages: Record<string, string> = {
  rs: 'rust', ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
  py: 'python', go: 'go', java: 'java', c: 'c', h: 'c', cc: 'cpp', cpp: 'cpp', hpp: 'cpp',
  json: 'json', md: 'markdown', markdown: 'markdown', toml: 'toml', yaml: 'yaml', yml: 'yaml',
  html: 'html', htm: 'html', css: 'css', scss: 'scss', less: 'less', xml: 'xml',
  sh: 'shell', bash: 'shell', sql: 'sql', php: 'php', rb: 'ruby', swift: 'swift', kt: 'kotlin',
};

export function extension(path: string) {
  return path.split('.').pop()?.toLowerCase() ?? '';
}

export function editorLanguage(path: string) {
  return languages[extension(path)] ?? 'plaintext';
}
