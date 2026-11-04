type RunOrder = { id: string; started_at: string };

function fractionalSeconds(timestamp: string): string {
  return (timestamp.match(/\.(\d+)/)?.[1] ?? '').padEnd(9, '0');
}

// Preserve the timestamp precision used by server pagination; IDs only break ties.
export function newestRunFirst(left: RunOrder, right: RunOrder): number {
  return Date.parse(right.started_at) - Date.parse(left.started_at)
    || fractionalSeconds(right.started_at).localeCompare(fractionalSeconds(left.started_at))
    || (left.id < right.id ? 1 : left.id > right.id ? -1 : 0);
}

export function runCursor(run: RunOrder): string {
  return `${run.started_at}/${run.id}`;
}
