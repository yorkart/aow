// Xterm parses ordinary writes in submission order. Submit them together;
// only reset/restore/status operations need to wait for their callbacks.
export class TerminalOutputQueue {
  private barrierTail: Promise<void> = Promise.resolve();
  private writes = new Set<Promise<unknown>>();

  write<T>(operation: () => T | Promise<T>): Promise<T> {
    const committed = this.barrierTail.then(operation);
    this.writes.add(committed);
    void committed.then(
      () => this.writes.delete(committed),
      () => this.writes.delete(committed),
    );
    return committed;
  }

  barrier<T>(operation: () => T | Promise<T>): Promise<T> {
    const committed = this.idle().then(operation);
    this.barrierTail = committed.then(() => {}, () => {});
    return committed;
  }

  idle(): Promise<void> {
    // A failed or skipped write must not let a reset overtake other writes
    // still parsing. Capture this boundary before subsequent writes arrive.
    return Promise.allSettled([this.barrierTail, ...this.writes]).then(() => {});
  }
}
