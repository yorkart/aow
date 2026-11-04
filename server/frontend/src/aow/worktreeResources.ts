export interface WorktreeResourceState {
  // Includes owned, floating, and hosted tabs. Undefined means restoration is incomplete.
  hasFrontendTabs: boolean | undefined;
  // All backend terminal instances count, regardless of origin, visibility, or process status.
  // Undefined means the terminal inventory is not yet known.
  hasTerminalInstances: boolean | undefined;
}

/** Unallocated requires both inventories to be confirmed empty; unknown is not empty. */
export function isWorktreeUnallocated(resources: WorktreeResourceState): boolean {
  return resources.hasFrontendTabs === false && resources.hasTerminalInstances === false;
}
