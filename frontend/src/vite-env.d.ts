/// <reference types="vite/client" />

interface Window {
  MonacoEnvironment?: {
    getWorker(moduleId: string, label: string): Worker;
  };
  requestIdleCallback?(callback: () => void, options?: { timeout?: number }): number;
  cancelIdleCallback?(handle: number): void;
}
