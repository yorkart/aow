export interface PullRequestDiffLine {
  kind: 'hunk' | 'added' | 'removed' | 'context' | 'meta';
  text: string;
  oldLine?: number;
  newLine?: number;
}

// Preserve hunk offsets: additions/deletions consume only their own side's line number.
export function pullRequestDiffLines(patch: string): PullRequestDiffLine[] {
  let oldLine = 0;
  let newLine = 0;
  let inHunk = false;
  const lines = patch.split('\n');
  if (lines.at(-1) === '') lines.pop();
  return lines.map((line) => {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
    if (hunk) {
      oldLine = Number(hunk[1]); newLine = Number(hunk[2]); inHunk = true;
      return { kind: 'hunk', text: line };
    }
    if (inHunk && line.startsWith('+')) return { kind: 'added', text: line.slice(1), newLine: newLine++ };
    if (inHunk && line.startsWith('-')) return { kind: 'removed', text: line.slice(1), oldLine: oldLine++ };
    if (inHunk && line.startsWith(' ')) return { kind: 'context', text: line.slice(1), oldLine: oldLine++, newLine: newLine++ };
    return { kind: 'meta', text: line };
  });
}
