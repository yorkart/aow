import type { SearchFile, SearchMatch } from './searchTypes';

export interface SearchNode {
  path: string;
  name: string;
  count: number;
  children: SearchNode[];
  file?: SearchFile;
}
export type SearchRow = { id: string; depth: number; node: SearchNode; match?: SearchMatch; parent?: string };

export function searchTree(root: string, files: SearchFile[]): SearchNode {
  const tree: SearchNode = { path: root, name: root.split('/').filter(Boolean).pop() ?? '/', count: 0, children: [] };
  const directories = new Map<string, SearchNode>([['', tree]]);
  for (const file of files) {
    const parts = file.relative_path.split('/');
    let parent = tree;
    const count = file.matches.length || 1;
    tree.count += count;
    for (let index = 0; index < parts.length - 1; index++) {
      const relative = parts.slice(0, index + 1).join('/');
      let node = directories.get(relative);
      if (!node) {
        node = { path: `${root.replace(/\/$/, '')}/${relative}`, name: parts[index], count: 0, children: [] };
        directories.set(relative, node);
        parent.children.push(node);
      }
      node.count += count;
      parent = node;
    }
    parent.children.push({ path: file.path, name: parts.at(-1) ?? file.path, count, children: [], file });
  }
  const sort = (node: SearchNode) => {
    node.children.sort((a, b) => Number(Boolean(a.file)) - Number(Boolean(b.file)) || a.name.localeCompare(b.name));
    node.children.forEach(sort);
  };
  sort(tree);
  return tree;
}

export function searchRows(tree: SearchNode, collapsed: ReadonlySet<string>): SearchRow[] {
  const rows: SearchRow[] = [];
  const append = (node: SearchNode, depth: number, parent?: string) => {
    rows.push({ id: node.path, node, depth, parent });
    if (collapsed.has(node.path)) return;
    for (const child of node.children) append(child, depth + 1, node.path);
    node.file?.matches.forEach((match, index) => rows.push({
      id: `${node.path}:${match.line}:${match.column}:${index}`, node, depth: depth + 1, match, parent: node.path,
    }));
  };
  append(tree, 0);
  return rows;
}
