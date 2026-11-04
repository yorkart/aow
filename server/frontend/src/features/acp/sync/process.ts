import { t } from '../../../lib/i18n';
import { useCallback, useEffect, useMemo, useSyncExternalStore } from 'react';
import { acpApi } from '../api';
import { applySessionUpdates } from '../session_updates';
import type { SessionSnapshot } from '../types';

interface State { data?: SessionSnapshot; error: string }
const processes = new Map<string, Process>();
class Process {
  state: State = { error: '' };
  private listeners = new Set<() => void>();
  private wanted = 0;
  private pending?: Promise<void>;
  private controller?: AbortController;
  private eviction: ReturnType<typeof setTimeout>;
  private key: string;
  constructor(readonly thread: string, readonly turn: string, readonly generation?: string) {
    this.key = JSON.stringify([thread, turn, generation]);
    // Concurrent renders may allocate a process without ever mounting it.
    this.eviction = setTimeout(() => { if (!this.listeners.size && processes.get(this.key) === this) processes.delete(this.key); }, 30_000);
  }
  snapshot = () => this.state;
  subscribe = (listener: () => void) => {
    clearTimeout(this.eviction);
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
      queueMicrotask(() => {
        if (this.listeners.size) return;
        this.controller?.abort(); this.state = { error: '' };
        if (processes.get(this.key) === this) processes.delete(this.key);
      });
    };
  };
  private publish(state: State) { this.state = state; for (const listener of this.listeners) listener(); }
  update(revision: number) { this.wanted = Math.max(this.wanted, revision); void this.read(); }
  reload = () => { this.publish({ error: '' }); void this.read(); };
  private read() {
    if (this.pending || !this.listeners.size || this.state.error || this.state.data && this.state.data.revision >= this.wanted) return;
    const controller = new AbortController(); this.controller = controller;
    const current = this.state.data;
    this.pending = (async () => {
      try {
        let next = applySessionUpdates(current, await acpApi.process(this.thread, this.turn, current, controller.signal));
        if (!next) next = applySessionUpdates(undefined, await acpApi.process(this.thread, this.turn, undefined, controller.signal));
        if (controller.signal.aborted) return;
        if (!next || next.sync_generation !== this.generation) throw new Error(t("acp.session_updated_expand_activity_again"));
        if (next.revision < this.wanted && next.revision === current?.revision) throw new Error(t("acp.activity_has_not_synced_yet_try_again"));
        this.publish({ data: next, error: '' });
      } catch (error) {
        if (!controller.signal.aborted) this.publish({ ...this.state, error: error instanceof Error ? error.message : String(error) });
      } finally {
        this.pending = undefined;
        if (!controller.signal.aborted) queueMicrotask(() => { void this.read(); });
      }
    })();
  }
}
export function useProcessSync(thread: string, turn: string, generation: string | undefined, revision: number) {
  const process = useMemo(() => {
    const key = JSON.stringify([thread, turn, generation]);
    let value = processes.get(key);
    if (!value) { value = new Process(thread, turn, generation); processes.set(key, value); }
    return value;
  }, [thread, turn, generation]);
  const subscribe = useCallback((listener: () => void) => process.subscribe(listener), [process]);
  const state = useSyncExternalStore(subscribe, process.snapshot);
  useEffect(() => { process.update(revision); }, [process, revision]);
  return { ...state, reload: process.reload };
}
