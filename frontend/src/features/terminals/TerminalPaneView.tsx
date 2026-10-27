import { t } from '../../lib/i18n';
import { appLocalStorage } from '../../lib/basePath';
import { getTheme, subscribeTheme } from '../../lib/theme/store';
import { resolveTerminalPalette, subscribeTerminalPalette } from '../../lib/theme/terminalPalette';
import { useEffect, useLayoutEffect, useRef, useState, type DragEvent, type RefObject } from 'react';
import { Base64, ClipboardAddon, type ClipboardSelectionType, type IBase64, type IClipboardProvider } from '@xterm/addon-clipboard';
import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';
import { ClipboardCopy, ExternalLink, RefreshCw, X } from 'lucide-react';
import { ConfirmationDialog } from '../../components/ConfirmationDialog';
import { terminalApi } from './terminalApi';
import { TerminalHostingDetails, hostingStatus } from './TerminalHostingStatus';
import type { TerminalPane, TerminalPaneStatus } from './types';
import { savedTerminalDimensions, type TerminalFrame, type TerminalPaneControls } from './terminalPresentation';
import { terminalPaneStatusMessage, type TerminalConnectionState } from './terminalState';
import { installMobileTerminalInput } from './terminalMobileInput';
import { installMobileTerminalRendering } from './terminalMobileRendering';
import { usePublishTerminalConnection } from './terminalViewState';
import { TerminalOutputQueue } from './terminalOutputQueue';
import { isCompressedTerminalSnapshot, writeCompressedTerminalSnapshot, type CompressedTerminalSnapshot } from './terminalSnapshot';
import { EXPLORER_PATH_MIME, readExplorerPath } from '../files/explorerDrag';
import '@xterm/xterm/css/xterm.css';
import './terminal.css';

// Keep reset replay bounded during rolling upgrades from the legacy daemon's
// 64 MiB scrollback. Live catch-up uses actual unparsed bytes, including those
// already submitted to xterm. A large total output or many small frames alone
// must not reset a consumer that is keeping up (Paseo's 4 MiB high-water mark).
const MAX_REPLAY_QUEUED_BINARY_BYTES = 80 * 1024 * 1024;
const MAX_LIVE_QUEUED_BINARY_BYTES = 4 * 1024 * 1024;
const MAX_SNAPSHOT_RECOVERY_REPLAY_BYTES = 1024 * 1024;
const SNAPSHOT_RECOVERY_COOLDOWN_MS = 3000;
// xterm time-slices parsing internally. Feeding one moderately sized chunk is
// substantially cheaper than putting thousands of tiny WebSocket frames into
// its write queue, while staying below the range where a single parse becomes
// visibly blocking.
const MAX_XTERM_WRITE_BATCH_BYTES = 256 * 1024;
// Interactive programs redraw the whole alternate screen on SIGWINCH. Wait for
// a resize gesture to settle so dragging a window does not generate dozens of
// obsolete full-screen redraws. Local xterm fit remains animation-frame paced.
const PTY_RESIZE_SETTLE_MS = 120;
// Serialized VT state is carried in the Stream control message so it does not
// advance the raw PTY byte offset. Match the worker's 8 MiB UTF-8 snapshot
// limit. The browser validates bytes independently because a malformed server
// response must not bypass the worker's authoritative limit with multibyte
// Unicode.
const MAX_VT_SNAPSHOT_BYTES = 8 * 1024 * 1024;
// Keep aligned with terminald's VT retention and full snapshot scrollback.
const TERMINAL_SCROLLBACK_LINES = 5_000;
const WAITING_RETRY_MIN_MS = 100;
const WAITING_RETRY_MAX_MS = 1000;
const DEFAULT_TERMINAL_LINE_HEIGHT = 1.15;
const TERMINAL_SCROLLBAR_WIDTH = 8;
const MAX_OSC52_TEXT_BYTES = 256 * 1024;
const MAX_OSC52_BASE64_CHARS = Math.ceil(MAX_OSC52_TEXT_BYTES / 3) * 4;
const CLIPBOARD_COPY_REQUEST_TTL_MS = 30_000;
const MAX_CLIPBOARD_IMAGE_BYTES = 10 * 1024 * 1024;
const SUPPORTED_CLIPBOARD_IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp']);
let primaryClipboardText = '';
let queuedTerminalBinaryBytes = 0;

type ConnectionState = TerminalConnectionState;
type ClipboardImageState = { status: 'uploading' | 'success' | 'error'; message: string } | null;
type ClipboardCopyState = { status: 'pending' | 'success' | 'error'; message: string; text?: string } | null;
type ControlMessage = {
  type: string;
  status?: TerminalPaneStatus;
  exit_code?: number | null;
  message?: string;
  code?: string;
  epoch?: string;
  offset?: number;
  reset?: boolean;
  replay_bytes?: number;
  // Preserve presence and the raw JSON type for strict Stream validation.
  // Silently dropping an invalid optional field would turn a malformed
  // snapshot into a seemingly valid legacy handshake.
  restore?: unknown;
  restore_gzip?: unknown;
  restore_cols?: unknown;
  restore_rows?: unknown;
  state?: 'claimed' | 'observing' | 'waiting';
};

interface Props {
  sizing?: 'container' | 'saved';
  renderer?: 'auto' | 'dom';
  fontSize?: number;
  attachEnabled?: boolean;
  forceOnAttach?: boolean;
  autoFocus?: boolean;
  focusRequest?: number;
  inputSuspended?: boolean;
  mobileInput?: boolean;
  controlsRef?: RefObject<TerminalPaneControls | null>;
  onConnectionChange?: (state: TerminalConnectionState, message?: string) => void;
  onFrameChange?: (frame: TerminalFrame) => void;
  visible: boolean;
  tabId: string;
  pane: TerminalPane;
  active: boolean;
  onFocus: () => void;
  onStatus: (status: TerminalPaneStatus, exitCode?: number | null) => void;
  onRebuild?: () => void;
  rebuilding?: boolean;
}

function controlMessage(value: string): ControlMessage | null {
  try {
    const payload = JSON.parse(value) as unknown;
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return null;
    const item = payload as Record<string, unknown>;
    if (typeof item.type !== 'string') return null;
    return {
      type: item.type,
      ...(item.status === 'running' || item.status === 'exited' || item.status === 'interrupted'
        ? { status: item.status }
        : {}),
      ...(typeof item.exit_code === 'number' || item.exit_code === null ? { exit_code: item.exit_code } : {}),
      ...(typeof item.message === 'string' ? { message: item.message } : {}),
      ...(typeof item.code === 'string' ? { code: item.code } : {}),
      ...(typeof item.epoch === 'string' && item.epoch.length > 0 ? { epoch: item.epoch } : {}),
      ...(Number.isSafeInteger(item.offset) && (item.offset as number) >= 0 ? { offset: item.offset as number } : {}),
      ...(typeof item.reset === 'boolean' ? { reset: item.reset } : {}),
      ...(Number.isSafeInteger(item.replay_bytes) && (item.replay_bytes as number) >= 0
        ? { replay_bytes: item.replay_bytes as number }
        : {}),
      ...(Object.prototype.hasOwnProperty.call(item, 'restore') ? { restore: item.restore } : {}),
      ...(Object.prototype.hasOwnProperty.call(item, 'restore_gzip') ? { restore_gzip: item.restore_gzip } : {}),
      ...(Object.prototype.hasOwnProperty.call(item, 'restore_cols') ? { restore_cols: item.restore_cols } : {}),
      ...(Object.prototype.hasOwnProperty.call(item, 'restore_rows') ? { restore_rows: item.restore_rows } : {}),
      ...(item.state === 'claimed' || item.state === 'observing' || item.state === 'waiting' ? { state: item.state } : {}),
      ...(Object.prototype.hasOwnProperty.call(item, 'tab') ? { tab: item.tab } : {}),
    };
  } catch {
    return null;
  }
}

type TerminalRestore = {
  data: string;
  gzip?: CompressedTerminalSnapshot;
  cols?: number;
  rows?: number;
};

type TerminalRestoreValidation =
  | { ok: true; restore?: TerminalRestore }
  | { ok: false; message: string; closeReason: string };

function terminalRestore(control: ControlMessage & { reset: boolean }): TerminalRestoreValidation {
  if (Object.prototype.hasOwnProperty.call(control, 'restore_gzip')) {
    if (Object.prototype.hasOwnProperty.call(control, 'restore')
      || !isCompressedTerminalSnapshot(control.restore_gzip)
      || !Object.prototype.hasOwnProperty.call(control, 'restore_cols')
      || !Object.prototype.hasOwnProperty.call(control, 'restore_rows')
      || typeof DecompressionStream === 'undefined') {
      return { ok: false, message: t("terminals.server_returned_invalid_terminal_recovery_data"), closeReason: 'invalid compressed terminal restore' };
    }
    const { restore_gzip: gzip, ...plain } = control;
    const validation = terminalRestore({ ...plain, restore: '' });
    if (validation.ok && validation.restore) validation.restore.gzip = gzip;
    return validation;
  }
  const hasRestore = Object.prototype.hasOwnProperty.call(control, 'restore');
  const hasRestoreCols = Object.prototype.hasOwnProperty.call(control, 'restore_cols');
  const hasRestoreRows = Object.prototype.hasOwnProperty.call(control, 'restore_rows');

  if (!hasRestore && !hasRestoreCols && !hasRestoreRows) return { ok: true };
  if (!hasRestore || typeof control.restore !== 'string') {
    return {
      ok: false,
      message: t("terminals.server_returned_invalid_terminal_recovery_data"),
      closeReason: 'invalid terminal restore data',
    };
  }
  if (control.restore.length > MAX_VT_SNAPSHOT_BYTES
    || utf8ByteLength(control.restore, MAX_VT_SNAPSHOT_BYTES) > MAX_VT_SNAPSHOT_BYTES) {
    return {
      ok: false,
      message: t("terminals.server_returned_oversized_terminal_recovery_data"),
      closeReason: 'terminal restore exceeds size limit',
    };
  }
  if (!control.reset) {
    return {
      ok: false,
      message: t("terminals.server_returned_invalid_terminal_recovery_data_in_the_resume"),
      closeReason: 'terminal restore without reset',
    };
  }
  if (hasRestoreCols !== hasRestoreRows) {
    return {
      ok: false,
      message: t("terminals.server_returned_incomplete_terminal_snapshot_dimensions"),
      closeReason: 'incomplete terminal snapshot dimensions',
    };
  }
  if (!hasRestoreCols) {
    // Legacy reset repair: mode sequences have no snapshot geometry.
    return { ok: true, restore: { data: control.restore } };
  }

  const cols = control.restore_cols;
  const rows = control.restore_rows;
  if (typeof cols !== 'number' || !Number.isInteger(cols) || cols < 1 || cols > 1000
    || typeof rows !== 'number' || !Number.isInteger(rows) || rows < 1 || rows > 1000) {
    return {
      ok: false,
      message: t("terminals.server_returned_invalid_terminal_snapshot_dimensions"),
      closeReason: 'invalid terminal snapshot dimensions',
    };
  }
  return { ok: true, restore: { data: control.restore, cols, rows } };
}

function utf8ByteLength(text: string, stopAfter = Number.MAX_SAFE_INTEGER) {
  let bytes = 0;
  for (let index = 0; index < text.length; index += 1) {
    const unit = text.charCodeAt(index);
    if (unit < 0x80) bytes += 1;
    else if (unit < 0x800) bytes += 2;
    else if (unit >= 0xd800 && unit <= 0xdbff
      && index + 1 < text.length
      && text.charCodeAt(index + 1) >= 0xdc00
      && text.charCodeAt(index + 1) <= 0xdfff) {
      bytes += 4;
      index += 1;
    } else bytes += 3;
    if (bytes > stopAfter) return bytes;
  }
  return bytes;
}

type TerminalRestoreDiagnostics = {
  startedAt: number;
  openedAt?: number;
  websocketOpenMs?: number;
  handshakeMs?: number;
  mode: 'resume' | 'vt_snapshot' | 'raw_replay';
  snapshotUsed: boolean;
  snapshotBytes: number;
  snapshotWriteMs: number;
  replayBytes: number;
  replayWriteMs: number;
  reported: boolean;
};

function terminalRestoreDebugEnabled() {
  try {
    return new URLSearchParams(window.location.search).get('terminalDebug') === '1'
      || appLocalStorage.getItem('terminal.debug') === '1';
  } catch {
    return false;
  }
}

function roundedMilliseconds(value: number | undefined) {
  return value === undefined ? undefined : Math.round(value * 10) / 10;
}

function terminalWrite(terminal: Terminal, data: string | Uint8Array) {
  return new Promise<void>((resolve) => terminal.write(data, resolve));
}

function terminalWriteln(terminal: Terminal, data: string) {
  return new Promise<void>((resolve) => terminal.writeln(data, resolve));
}

type XtermRenderDimensions = {
  device: { char: { height: number }; cell: { height: number } };
};

type XtermWithRenderDimensions = Terminal & {
  _core?: { _renderService?: { dimensions?: XtermRenderDimensions } };
};

function fitTerminal(terminal: Terminal, fit: FitAddon, host: HTMLElement) {
  const dimensions = (terminal as XtermWithRenderDimensions)._core?._renderService?.dimensions;
  const deviceCharHeight = dimensions?.device.char.height ?? 0;
  const availableHeight = parseInt(window.getComputedStyle(host).height, 10);
  const dpr = window.devicePixelRatio;

  if (deviceCharHeight > 0 && availableHeight > 0 && Number.isFinite(dpr) && dpr > 0) {
    const defaultDeviceCellHeight = Math.floor(deviceCharHeight * DEFAULT_TERMINAL_LINE_HEIGHT);
    // xterm's DOM renderer rounds cell height to whole device pixels. FitAddon
    // then floors the row count, which can otherwise leave almost a full row
    // blank at the bottom. Search only one CSS pixel around the default cell
    // height and use the candidate whose final canvas leaves the least space.
    const devicePixelRange = Math.max(1, Math.ceil(dpr));
    let bestDeviceCellHeight = defaultDeviceCellHeight;
    let bestGap = Number.POSITIVE_INFINITY;
    let bestDistance = Number.POSITIVE_INFINITY;

    for (let deviceCellHeight = Math.max(deviceCharHeight, defaultDeviceCellHeight - devicePixelRange);
      deviceCellHeight <= defaultDeviceCellHeight + devicePixelRange;
      deviceCellHeight += 1) {
      const rows = Math.max(1, Math.floor(availableHeight * dpr / deviceCellHeight));
      const canvasHeight = Math.round(deviceCellHeight * rows / dpr);
      const gap = Math.max(0, availableHeight - canvasHeight);
      const distance = Math.abs(deviceCellHeight - defaultDeviceCellHeight);
      if (gap < bestGap || (gap === bestGap && distance < bestDistance)) {
        bestDeviceCellHeight = deviceCellHeight;
        bestGap = gap;
        bestDistance = distance;
      }
    }

    const lineHeight = bestDeviceCellHeight === defaultDeviceCellHeight
      ? DEFAULT_TERMINAL_LINE_HEIGHT
      // Stay just inside the desired floor bucket despite floating point error.
      : (bestDeviceCellHeight + 0.001) / deviceCharHeight;
    if (terminal.options.lineHeight !== lineHeight) terminal.options.lineHeight = lineHeight;
  }

  const proposed = fit.proposeDimensions();
  const deviceCellHeight = dimensions?.device.cell.height ?? 0;
  if (proposed && deviceCellHeight > 0 && availableHeight > 0 && Number.isFinite(dpr) && dpr > 0) {
    // The DOM renderer derives its CSS cell height from a rounded canvas height
    // divided by the current row count. Using that value to fit again makes the
    // result depend on the previous grid (e.g. 43 -> 29 -> 42 rows after zoom).
    // Fit rows against the actual device-pixel cell height instead.
    const style = window.getComputedStyle(terminal.element!);
    const padding = (parseFloat(style.paddingTop) || 0) + (parseFloat(style.paddingBottom) || 0);
    const rows = Math.max(1, Math.floor((availableHeight - padding) * dpr / deviceCellHeight));
    terminal.resize(proposed.cols, rows);
  } else fit.fit();
}

function clipboardImage(data: DataTransfer | null): File | undefined {
  if (!data) return undefined;
  for (const item of Array.from(data.items)) {
    if (item.kind === 'file' && (!item.type || item.type.toLowerCase().startsWith('image/'))) {
      const file = item.getAsFile();
      if (file && (!file.type || file.type.toLowerCase().startsWith('image/'))) return file;
    }
  }
  return Array.from(data.files).find((file) => !file.type || file.type.toLowerCase().startsWith('image/'));
}

function reasonMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : t("terminals.unknown_error");
}

function quotePastedPath(path: string) {
  return `'${path.replace(/'/g, "'\\''")}'`;
}

function textByteLength(text: string) {
  return new TextEncoder().encode(text).byteLength;
}

async function writeTerminalClipboard(text: string, isCurrent: () => boolean) {
  if (!isCurrent()) return false;
  if (window.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch { /* Try the selection-based API while the user gesture is still active. */ }
  }
  // LAN HTTP has no Async Clipboard API. OSC 52 arrives after the terminal
  // application's mouse/keyboard response, but browsers may still allow copy
  // using the original gesture. Only ask for another click if both APIs fail.
  return isCurrent() && copyWithTextArea(text);
}

function copyWithTextArea(text: string) {
  const activeElement = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.readOnly = true;
  textarea.tabIndex = -1;
  textarea.setAttribute('aria-hidden', 'true');
  textarea.style.position = 'fixed';
  textarea.style.left = '-10000px';
  textarea.style.top = '0';
  document.body.appendChild(textarea);
  textarea.focus({ preventScroll: true });
  textarea.select();
  let copied = false;
  try { copied = document.execCommand('copy'); } catch { /* Browser denied the fallback. */ }
  textarea.remove();
  activeElement?.focus({ preventScroll: true });
  return copied;
}

class LimitedClipboardBase64 implements IBase64 {
  private readonly base64 = new Base64();

  encodeText(data: string) {
    if (textByteLength(data) > MAX_OSC52_TEXT_BYTES) throw new Error(t("common.osc_52_response_is_too_large"));
    return this.base64.encodeText(data);
  }

  decodeText(data: string) {
    if (data.length > MAX_OSC52_BASE64_CHARS) throw new Error(t("common.osc_52_payload_is_too_large"));
    const text = this.base64.decodeText(data);
    if (textByteLength(text) > MAX_OSC52_TEXT_BYTES) throw new Error(t("common.osc_52_decoded_text_is_too_large"));
    return text;
  }
}

export function TerminalPaneView({ visible, tabId, pane, active, onFocus, onStatus, onRebuild, rebuilding = false,
  sizing = 'container', renderer = 'auto', fontSize = 13, attachEnabled = true, forceOnAttach = false, autoFocus = true, focusRequest = 0, inputSuspended = false, mobileInput = false, controlsRef, onConnectionChange, onFrameChange,
}: Props) {
  const [connection, setConnection] = useState<ConnectionState>(pane.status === 'running' ? 'connecting' : pane.status);
  usePublishTerminalConnection(tabId, pane.id, attachEnabled && pane.status === 'running' ? connection
    : pane.status === 'running' ? 'disconnected' : pane.status);
  if (pane.agent_terminal && connection !== 'connected') sizing = 'saved';
  const agentPhaseRef = useRef(pane.agent_terminal?.phase);
  agentPhaseRef.current = pane.agent_terminal?.phase;
  const presentationRef = useRef({ sizing, onFrameChange });
  presentationRef.current = { sizing, onFrameChange };
  const savedSizeRef = useRef(savedTerminalDimensions(pane));
  savedSizeRef.current = savedTerminalDimensions(pane);
  const shellRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const appliedFocusRequest = useRef(0);
  const terminalRef = useRef<Terminal | undefined>(undefined);
  const stdinDisabled = inputSuspended || Boolean(pane.hosting);
  const inputSuspendedRef = useRef(stdinDisabled);
  inputSuspendedRef.current = stdinDisabled;
  const fitRef = useRef<FitAddon | undefined>(undefined);
  const socketRef = useRef<WebSocket | undefined>(undefined);
  const inputReadySocketRef = useRef<WebSocket | undefined>(undefined);
  const pendingResizeRef = useRef<{ cols: number; rows: number } | undefined>(undefined);
  const resizeSendTimerRef = useRef<number | undefined>(undefined);
  const lastSentResizeRef = useRef<{ socket: WebSocket; cols: number; rows: number } | undefined>(undefined);
  const mutationGenerationRef = useRef(0);
  const manualReconnectRef = useRef<(force?: boolean) => void>(() => {});
  const terminalOperationQueueRef = useRef(new TerminalOutputQueue());
  const callbackRef = useRef({ onFocus, onStatus });
  const paneStatusRef = useRef(pane.status);
  const renderedStatusRef = useRef<string | undefined>(undefined);
  const paneIdentity = `${tabId}\u0000${pane.id}`;
  const activeIdentityRef = useRef(paneIdentity);
  const renderedIdentityRef = useRef(paneIdentity);
  const streamStateRef = useRef({ identity: paneIdentity, initialized: false, epoch: '', nextOffset: 0 });
  const clipboardContextRef = useRef({ active, visible });
  const pendingClipboardPathsRef = useRef<string[]>([]);
  const clipboardSuccessTimerRef = useRef<number | undefined>(undefined);
  const clipboardCopyTimerRef = useRef<number | undefined>(undefined);
  const clipboardCopyRequestRef = useRef(0);
  const clipboardComponentAliveRef = useRef(true);
  const [connectionMessage, setConnectionMessage] = useState<string | undefined>(undefined);
  const [restorePending, setRestorePendingState] = useState(pane.status === 'running');
  const [clipboardImageState, setClipboardImageState] = useState<ClipboardImageState>(null);
  const [clipboardCopyState, setClipboardCopyState] = useState<ClipboardCopyState>(null);
  const [pendingLink, setPendingLink] = useState<{ identity: string; url: URL } | null>(null);
  const [pathDropActive, setPathDropActive] = useState(false);
  const setRestorePending = (pending: boolean) => {
    // Apply visibility synchronously before feeding reset/replay bytes into
    // xterm. React will retain the class on the next render.
    shellRef.current?.classList.toggle('restore-pending', pending);
    setRestorePendingState(pending);
  };

  callbackRef.current = { onFocus, onStatus };
  paneStatusRef.current = pane.status;
  clipboardContextRef.current = { active, visible };

  useLayoutEffect(() => {
    if (activeIdentityRef.current === paneIdentity) return;
    activeIdentityRef.current = paneIdentity;
    streamStateRef.current = { identity: paneIdentity, initialized: false, epoch: '', nextOffset: 0 };
    setRestorePending(pane.status === 'running');
    mutationGenerationRef.current += 1;
    inputReadySocketRef.current = undefined;
    if (clipboardCopyTimerRef.current !== undefined) {
      window.clearTimeout(clipboardCopyTimerRef.current);
      clipboardCopyTimerRef.current = undefined;
    }
    setClipboardCopyState(null);
    setPendingLink(null);
  }, [paneIdentity]);

  useEffect(() => {
    if (!visible) setPendingLink(null);
  }, [visible]);

  useEffect(() => {
    clipboardComponentAliveRef.current = true;
    return () => { clipboardComponentAliveRef.current = false; };
  }, []);

  const clipboardPaneIsCurrent = (requireTerminalFocus: boolean) => {
    const host = hostRef.current;
    if (!clipboardContextRef.current.active || !clipboardContextRef.current.visible || !host) return false;
    if (document.visibilityState !== 'visible' || !document.hasFocus()) return false;
    if (!requireTerminalFocus) return true;
    const activeElement = document.activeElement;
    return activeElement !== null && host.contains(activeElement);
  };

  const terminalCanMutate = () => {
    const socket = socketRef.current;
    return socket !== undefined
      && socket === inputReadySocketRef.current
      && socket.readyState === WebSocket.OPEN;
  };

  const canDropPath = () => visible && !inputSuspended && pane.status === 'running' && terminalCanMutate();
  const dragExplorerPath = (event: DragEvent<HTMLDivElement>) => {
    // Drag data is protected until drop; inspect only the type while hovering.
    if (!event.dataTransfer.types.includes(EXPLORER_PATH_MIME)) return;
    event.preventDefault();
    event.stopPropagation();
    const ready = canDropPath();
    event.dataTransfer.dropEffect = ready ? 'copy' : 'none';
    setPathDropActive(ready);
  };
  const dropExplorerPath = (event: DragEvent<HTMLDivElement>) => {
    if (!event.dataTransfer.types.includes(EXPLORER_PATH_MIME)) return;
    event.preventDefault();
    event.stopPropagation();
    setPathDropActive(false);
    const path = readExplorerPath(event.dataTransfer);
    const terminal = terminalRef.current;
    if (!path || !terminal || !canDropPath()) return;
    onFocus();
    terminal.focus();
    terminal.paste(`${quotePastedPath(path)} `);
  };

  useEffect(() => { setPathDropActive(false); }, [visible, inputSuspended, connection, paneIdentity]);

  const reportFrame = () => {
    if (presentationRef.current.sizing !== 'saved' && !presentationRef.current.onFrameChange) return;
    const terminal = terminalRef.current;
    const host = hostRef.current;
    const screen = host?.querySelector<HTMLElement>('.xterm-screen');
    if (!terminal || !host || !screen || !screen.offsetWidth || !screen.offsetHeight) return;
    const width = screen.offsetWidth + TERMINAL_SCROLLBAR_WIDTH;
    const height = screen.offsetHeight;
    if (presentationRef.current.sizing === 'saved') {
      host.style.width = `${width}px`;
      host.style.height = `${height}px`;
    }
    const buffer = terminal.buffer.active;
    presentationRef.current.onFrameChange?.({ cols: terminal.cols, rows: terminal.rows, width, height, top: screen.offsetTop,
      cursorRow: buffer.baseY + buffer.cursorY - buffer.viewportY });
  };

  useEffect(() => { onConnectionChange?.(connection, connectionMessage); }, [connection, connectionMessage, onConnectionChange]);

  useLayoutEffect(() => {
    if (sizing === 'container' && hostRef.current) {
      hostRef.current.style.removeProperty('width');
      hostRef.current.style.removeProperty('height');
    }
  }, [sizing]);

  useEffect(() => {
    if (sizing !== 'saved' || terminalCanMutate() || restorePending) return;
    const size = savedSizeRef.current;
    terminalRef.current?.resize(size.cols, size.rows);
    reportFrame();
  }, [pane.cols, pane.rows, sizing]);

  useLayoutEffect(() => {
    if (!controlsRef) return;
    controlsRef.current = {
      send: (text, paste = false) => {
        if (!terminalCanMutate() || !clipboardContextRef.current.active || !clipboardContextRef.current.visible
          || document.visibilityState !== 'visible') return false;
        if (paste) terminalRef.current?.paste(text);
        else {
          // eslint-disable-next-line no-control-regex -- Validate or match literal protocol control bytes intentionally.
          const data = terminalRef.current?.modes.applicationCursorKeysMode && /^\u001b\[[ABCD]$/.test(text)
            ? text.replace('[', 'O') : text;
          socketRef.current!.send(new TextEncoder().encode(data));
        }
        return true;
      },
      submit: (text) => {
        const terminal = terminalRef.current;
        if (!terminal || !terminalCanMutate() || !clipboardContextRef.current.active || !clipboardContextRef.current.visible
          || document.visibilityState !== 'visible') return false;
        // Match xterm's paste encoding, then submit outside the paste markers.
        // Send directly because keyboard input is suspended while the editor owns focus.
        const pasted = text.replace(/\r?\n/g, '\r');
        const data = terminal.modes.bracketedPasteMode && terminal.options.ignoreBracketedPasteMode !== true
          ? `\u001b[200~${pasted}\u001b[201~\r` : `${pasted}\r`;
        socketRef.current!.send(new TextEncoder().encode(data));
        return true;
      },
      takeControl: () => { if (!agentPhaseRef.current || agentPhaseRef.current === 'ready') manualReconnectRef.current(true); },
      focus: () => { if (terminalCanMutate()) terminalRef.current?.focus(); },
      scroll: (lines, point) => {
        const terminal = terminalRef.current;
        if (!terminal) return;
        if (lines === 'bottom') { terminal.scrollToBottom(); return; }
        if (!Number.isFinite(lines) || !Math.trunc(lines)) return;
        const mouse = terminal.modes.mouseTrackingMode;
        if (terminal.buffer.active.type !== 'alternate' && (mouse === 'none' || mouse === 'x10')) {
          terminal.scrollLines(Math.trunc(lines));
          return;
        }
        if (!terminalCanMutate() || !clipboardContextRef.current.active || !clipboardContextRef.current.visible
          || document.visibilityState !== 'visible') return;
        const screen = hostRef.current?.querySelector<HTMLElement>('.xterm-screen');
        if (!terminal.element || !screen) return;
        const bounds = screen.getBoundingClientRect();
        if (!bounds.width || !bounds.height) return;
        // xterm expects unscaled coordinates and owns mouse encoding / alternate-screen keys.
        const x = point ? Math.max(0, Math.min(1, (point.clientX - bounds.left) / bounds.width)) : .5;
        const y = point ? Math.max(0, Math.min(1, (point.clientY - bounds.top) / bounds.height)) : .5;
        for (let index = 0; index < Math.min(32, Math.abs(Math.trunc(lines))); index += 1) {
          terminal.element.dispatchEvent(new WheelEvent('wheel', {
            bubbles: false, cancelable: true, deltaMode: WheelEvent.DOM_DELTA_LINE, deltaY: Math.sign(lines),
            clientX: bounds.left + x * screen.offsetWidth, clientY: bounds.top + y * screen.offsetHeight,
          }));
        }
      },
    };
    return () => { controlsRef.current = null; };
  }, [controlsRef, paneIdentity]);

  const cancelPendingTerminalResize = () => {
    pendingResizeRef.current = undefined;
    if (resizeSendTimerRef.current !== undefined) {
      window.clearTimeout(resizeSendTimerRef.current);
      resizeSendTimerRef.current = undefined;
    }
  };

  const revokeInput = (socket?: WebSocket) => {
    if (socket !== undefined && inputReadySocketRef.current !== socket) return;
    mutationGenerationRef.current += 1;
    inputReadySocketRef.current = undefined;
    cancelPendingTerminalResize();
  };

  const flushTerminalResize = (cols: number, rows: number) => {
    if (presentationRef.current.sizing === 'saved') return;
    const socket = socketRef.current;
    const host = hostRef.current;
    if (!clipboardContextRef.current.visible || !host || host.clientWidth <= 0 || host.clientHeight <= 0
      || !socket || socket !== inputReadySocketRef.current || socket.readyState !== WebSocket.OPEN) return;
    const previous = lastSentResizeRef.current;
    if (previous?.socket === socket && previous.cols === cols && previous.rows === rows) return;
    socket.send(terminalApi.resizeMessage(cols, rows));
    lastSentResizeRef.current = { socket, cols, rows };
  };

  const sendTerminalResizeNow = (cols: number, rows: number) => {
    cancelPendingTerminalResize();
    flushTerminalResize(cols, rows);
  };

  const scheduleTerminalResize = (cols: number, rows: number) => {
    if (presentationRef.current.sizing === 'saved') return;
    pendingResizeRef.current = { cols, rows };
    if (resizeSendTimerRef.current !== undefined) window.clearTimeout(resizeSendTimerRef.current);
    resizeSendTimerRef.current = window.setTimeout(() => {
      resizeSendTimerRef.current = undefined;
      const pending = pendingResizeRef.current;
      pendingResizeRef.current = undefined;
      if (pending) flushTerminalResize(pending.cols, pending.rows);
    }, PTY_RESIZE_SETTLE_MS);
  };

  const clearClipboardCopyTimer = () => {
    if (clipboardCopyTimerRef.current !== undefined) {
      window.clearTimeout(clipboardCopyTimerRef.current);
      clipboardCopyTimerRef.current = undefined;
    }
  };
  const showClipboardCopySuccess = () => {
    clearClipboardCopyTimer();
    setClipboardCopyState({ status: 'success', message: t("terminals.copied_to_clipboard") });
    clipboardCopyTimerRef.current = window.setTimeout(() => {
      setClipboardCopyState(null);
      clipboardCopyTimerRef.current = undefined;
    }, 1600);
  };
  const showPendingClipboardCopy = (text: string) => {
    clearClipboardCopyTimer();
    setClipboardCopyState({ status: 'pending', message: t("terminals.terminal_requests_copying_characters", { value0: text.length }), text });
    clipboardCopyTimerRef.current = window.setTimeout(() => {
      setClipboardCopyState(null);
      clipboardCopyTimerRef.current = undefined;
    }, CLIPBOARD_COPY_REQUEST_TTL_MS);
  };
  const completePendingClipboardCopy = async () => {
    const text = clipboardCopyState?.text;
    if (!text || !clipboardPaneIsCurrent(/*requireTerminalFocus=*/ false)) return;
    const identity = activeIdentityRef.current;
    const request = ++clipboardCopyRequestRef.current;
    // This is an explicit user action on already received text. Copying does
    // not mutate the PTY and remains available after exit or loss of control.
    const isCurrent = () => clipboardComponentAliveRef.current
      && activeIdentityRef.current === identity
      && clipboardCopyRequestRef.current === request
      && clipboardPaneIsCurrent(/*requireTerminalFocus=*/ false);
    const copied = await writeTerminalClipboard(text, isCurrent);
    if (!isCurrent()) return;
    if (copied) showClipboardCopySuccess();
    else setClipboardCopyState({ status: 'error', message: t("terminals.the_browser_denied_clipboard_access_check_site_permissions"), text });
    terminalRef.current?.focus();
  };

  const clearClipboardSuccessTimer = () => {
    if (clipboardSuccessTimerRef.current !== undefined) {
      window.clearTimeout(clipboardSuccessTimerRef.current);
      clipboardSuccessTimerRef.current = undefined;
    }
  };
  const showClipboardSuccess = (count: number) => {
    clearClipboardSuccessTimer();
    setClipboardImageState({ status: 'success', message: count === 1 ? t("terminals.image_pasted") : t("terminals.images_pasted", { value0: count }) });
    clipboardSuccessTimerRef.current = window.setTimeout(() => {
      setClipboardImageState(null);
      clipboardSuccessTimerRef.current = undefined;
    }, 1600);
  };
  const flushClipboardPaths = () => {
    const terminal = terminalRef.current;
    const socket = socketRef.current;
    if (!terminal || !socket || socket !== inputReadySocketRef.current || socket.readyState !== WebSocket.OPEN) return 0;
    if (!clipboardContextRef.current.visible || !clipboardContextRef.current.active) return 0;
    const paths = pendingClipboardPathsRef.current.splice(0);
    for (const path of paths) terminal.paste(`${quotePastedPath(path)} `);
    return paths.length;
  };

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    host.style.setProperty('--terminal-scrollbar-width', `${TERMINAL_SCROLLBAR_WIDTH}px`);
    const terminalTheme = () => {
      const colors = resolveTerminalPalette(getTheme().appearance);
      return { ...colors, cursorAccent: colors.background, overviewRulerBorder: '#00000000' };
    };
    const terminal = new Terminal({
      ...(sizing === 'saved' ? savedSizeRef.current : {}),
      // Match VS Code's browser terminal width/rendering path. Unicode 11
      // provides the cell-width table, while WebGL can rescale fallback-font
      // glyphs whose painted width would otherwise overlap adjacent cells.
      allowProposedApi: true,
      disableStdin: inputSuspendedRef.current,
      cursorBlink: true,
      cursorStyle: 'block',
      fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace",
      fontSize,
      lineHeight: DEFAULT_TERMINAL_LINE_HEIGHT,
      overviewRuler: { width: TERMINAL_SCROLLBAR_WIDTH },
      rescaleOverlappingGlyphs: true,
      scrollback: TERMINAL_SCROLLBACK_LINES,
      rightClickSelectsWord: false,
      linkHandler: {
        activate(event, uri) {
          event.preventDefault();
          // Let mouseup reach xterm's document listeners so selection drags end
          // and mouse-reporting applications receive the button release.
          if (!clipboardContextRef.current.visible) return;
          let url: URL;
          try { url = new URL(uri); } catch { return; }
          if (url.protocol !== 'http:' && url.protocol !== 'https:') return;
          // Replace xterm's blocking window.confirm with the workspace dialog.
          setPendingLink((current) => current ?? { identity: activeIdentityRef.current, url });
        },
      },
      theme: terminalTheme(),
    });
    // Updating xterm options preserves its screen, selection, scrollback and socket.
    const updateTerminalTheme = () => { terminal.options.theme = terminalTheme(); };
    const unsubscribeTheme = subscribeTheme(updateTerminalTheme);
    const unsubscribePalette = subscribeTerminalPalette(updateTerminalTheme);
    const fit = new FitAddon();
    let disposed = false;
    const clipboardProvider: IClipboardProvider = {
      readText(selection: ClipboardSelectionType) {
        if (!terminalCanMutate() || !clipboardPaneIsCurrent(/*requireTerminalFocus=*/ true)) return '';
        // OSC 52 reads would expose the local system clipboard to a remote
        // process. PRIMARY remains an in-memory compatibility buffer only.
        return selection === 'p' ? primaryClipboardText : '';
      },
      writeText(selection: ClipboardSelectionType, text: string) {
        if (disposed || !terminalCanMutate() || !clipboardPaneIsCurrent(/*requireTerminalFocus=*/ true)) return;
        if (selection === 'p') {
          if (textByteLength(text) <= MAX_OSC52_TEXT_BYTES) primaryClipboardText = text;
          return;
        }
        if (selection !== 'c' || text.length === 0) return;
        const bytes = textByteLength(text);
        if (bytes > MAX_OSC52_TEXT_BYTES) {
          setClipboardCopyState({ status: 'error', message: t("terminals.terminal_copy_content_too_large_maximum_kib", { value0: MAX_OSC52_TEXT_BYTES / 1024 }) });
          return;
        }
        const identity = activeIdentityRef.current;
        const mutationGeneration = mutationGenerationRef.current;
        const request = ++clipboardCopyRequestRef.current;
        const isCurrent = () => !disposed
          && activeIdentityRef.current === identity
          && mutationGenerationRef.current === mutationGeneration
          && clipboardCopyRequestRef.current === request
          && terminalCanMutate()
          && clipboardPaneIsCurrent(/*requireTerminalFocus=*/ true);
        clearClipboardCopyTimer();
        setClipboardCopyState(null);
        void writeTerminalClipboard(text, isCurrent).then((copied) => {
          if (!isCurrent()) return;
          if (copied) showClipboardCopySuccess();
          else showPendingClipboardCopy(text);
        });
      },
    };
    terminal.loadAddon(fit);
    terminal.loadAddon(new ClipboardAddon(new LimitedClipboardBase64(), clipboardProvider));
    terminal.loadAddon(new Unicode11Addon());
    terminal.unicode.activeVersion = '11';
    terminal.open(host);
    const disposeMobileRendering = mobileInput && renderer === 'dom' ? installMobileTerminalRendering(terminal) : undefined;
    terminalRef.current = terminal;
    fitRef.current = fit;

    let frame: number | undefined;
    const resize = () => {
      if (frame !== undefined) return;
      frame = window.requestAnimationFrame(() => {
        frame = undefined;
        if (host.clientWidth > 0 && host.clientHeight > 0) {
          try { if (presentationRef.current.sizing === 'saved') reportFrame(); else fitTerminal(terminal, fit, host); } catch { /* xterm may still be measuring its font. */ }
        }
      });
    };

    // VS Code prefers the WebGL renderer and falls back to xterm's DOM
    // renderer when GPU setup fails. The overlap-rescaling option above only
    // takes effect in a canvas/WebGL renderer, so a CSS gutter cannot replace
    // this step. Refit after renderer changes because its measured cell size
    // can differ from the DOM renderer.
    let webgl: WebglAddon | undefined;
    try {
      // Mobile WebViews can opt into DOM rendering independently of their sizing policy.
      if (sizing === 'container' && renderer !== 'dom') {
        webgl = new WebglAddon();
        terminal.loadAddon(webgl);
        webgl.onContextLoss(() => {
          webgl?.dispose();
          webgl = undefined;
          resize();
        });
      }
    } catch (reason) {
      webgl?.dispose();
      webgl = undefined;
      console.warn('WebGL terminal renderer unavailable; using DOM renderer', reason);
    }
    const observer = new ResizeObserver(resize);
    observer.observe(host);
    resize();

    const input = terminal.onData((data) => {
      const socket = socketRef.current;
      if (socket && socket === inputReadySocketRef.current && socket.readyState === WebSocket.OPEN) {
        socket.send(new TextEncoder().encode(data));
      }
    });
    const disposeMobileInput = mobileInput ? installMobileTerminalInput(terminal) : undefined;
    const binaryInput = terminal.onBinary((data) => {
      const socket = socketRef.current;
      if (!socket || socket !== inputReadySocketRef.current || socket.readyState !== WebSocket.OPEN) return;
      const bytes = new Uint8Array(data.length);
      for (let index = 0; index < data.length; index += 1) bytes[index] = data.charCodeAt(index) & 0xff;
      socket.send(bytes);
    });
    const resizeEvent = terminal.onResize(({ cols, rows }) => {
      reportFrame();
      scheduleTerminalResize(cols, rows);
    });
    const renderEvent = terminal.onRender(reportFrame);
    const cursorEvent = terminal.onCursorMove(reportFrame);
    const focusEvent = () => callbackRef.current.onFocus();
    host.addEventListener('focusin', focusEvent);

    return () => {
      disposed = true;
      unsubscribeTheme();
      unsubscribePalette();
      observer.disconnect();
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      input.dispose();
      disposeMobileInput?.();
      disposeMobileRendering?.();
      binaryInput.dispose();
      resizeEvent.dispose();
      renderEvent.dispose();
      cursorEvent.dispose();
      host.removeEventListener('focusin', focusEvent);
      terminal.dispose();
      terminalRef.current = undefined;
      fitRef.current = undefined;
      cancelPendingTerminalResize();
      clearClipboardCopyTimer();
    };
  }, []);

  useLayoutEffect(() => {
    if (terminalRef.current) terminalRef.current.options.disableStdin = stdinDisabled;
  }, [stdinDisabled]);

  useEffect(() => {
    if (!active || !visible || restorePending) return;
    const terminal = terminalRef.current;
    if (!terminal || terminal.options.fontSize === fontSize) return;
    const frame = window.requestAnimationFrame(() => {
      terminal.options.fontSize = fontSize;
      const host = hostRef.current;
      const fit = fitRef.current;
      if (sizing === 'container' && host && fit && host.clientWidth > 0 && host.clientHeight > 0) {
        try { fitTerminal(terminal, fit, host); } catch { /* Font metrics may still be settling. */ }
      }
      reportFrame();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [fontSize, active, visible, restorePending, sizing]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let disposed = false;
    let queue = Promise.resolve();
    let currentUpload: AbortController | undefined;
    setClipboardImageState(null);

    const paste = (event: ClipboardEvent) => {
      const image = clipboardImage(event.clipboardData);
      if (!image) return;
      if (!clipboardPaneIsCurrent(/*requireTerminalFocus=*/ true)) return;

      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();

      clearClipboardSuccessTimer();
      const imageType = image.type.toLowerCase();
      if (imageType && !SUPPORTED_CLIPBOARD_IMAGE_TYPES.has(imageType)) {
        setClipboardImageState({ status: 'error', message: t("terminals.only_png_jpeg_gif_or_webp_images_are_supported") });
        return;
      }
      if (image.size === 0) {
        setClipboardImageState({ status: 'error', message: t("terminals.clipboard_image_is_empty") });
        return;
      }
      if (image.size > MAX_CLIPBOARD_IMAGE_BYTES) {
        setClipboardImageState({ status: 'error', message: t("terminals.clipboard_image_must_not_exceed_10_mib") });
        return;
      }
      if (socketRef.current !== inputReadySocketRef.current || socketRef.current?.readyState !== WebSocket.OPEN) {
        setClipboardImageState({ status: 'error', message: t("terminals.terminal_disconnected_cannot_paste_image") });
        return;
      }

      queue = queue.then(async () => {
        if (disposed) return;
        if (!clipboardPaneIsCurrent(/*requireTerminalFocus=*/ true)) return;
        if (socketRef.current !== inputReadySocketRef.current || socketRef.current?.readyState !== WebSocket.OPEN) return;
        clearClipboardSuccessTimer();
        const uploadSocket = inputReadySocketRef.current;
        const uploadGeneration = mutationGenerationRef.current;
        currentUpload = new AbortController();
        setClipboardImageState({ status: 'uploading', message: t("terminals.uploading_clipboard_image") });
        try {
          const response = await terminalApi.uploadClipboardImage(tabId, pane.id, image, currentUpload.signal);
          if (disposed
            || mutationGenerationRef.current !== uploadGeneration
            || inputReadySocketRef.current !== uploadSocket
            || uploadSocket?.readyState !== WebSocket.OPEN) return;
          pendingClipboardPathsRef.current.push(response.path);
          const pasted = flushClipboardPaths();
          if (pasted) showClipboardSuccess(pasted);
          else setClipboardImageState({ status: 'success', message: t("terminals.image_uploaded_switch_back_to_this_pane_to_attach") });
        } catch (reason) {
          if (!disposed && !(reason instanceof DOMException && reason.name === 'AbortError')) {
            setClipboardImageState({ status: 'error', message: t("terminals.image_upload_failed", { value0: reasonMessage(reason) }) });
          }
        } finally {
          currentUpload = undefined;
        }
      });
    };

    host.addEventListener('paste', paste, true);
    return () => {
      disposed = true;
      currentUpload?.abort();
      pendingClipboardPathsRef.current = [];
      clearClipboardSuccessTimer();
      clearClipboardCopyTimer();
      host.removeEventListener('paste', paste, true);
    };
  }, [active, pane.id, tabId, visible]);

  useEffect(() => {
    if (!active || !visible || connection !== 'connected') return;
    const pasted = flushClipboardPaths();
    if (pasted) showClipboardSuccess(pasted);
  }, [active, connection, visible]);

  useEffect(() => {
    // Deactivating a pane must not emit a redundant PTY resize. In particular,
    // maximizing another pane keeps this pane's layout box unchanged so it can
    // be restored without disturbing full-screen programs.
    if (!visible || !active || sizing === 'saved') return;
    const frame = window.requestAnimationFrame(() => {
      const host = hostRef.current;
      if (host && host.clientWidth > 0 && host.clientHeight > 0) {
        const terminal = terminalRef.current;
        const fit = fitRef.current;
        if (terminal && fit) {
          try { fitTerminal(terminal, fit, host); } catch { /* The visible container may still be settling. */ }
        }
        if (terminal) sendTerminalResizeNow(terminal.cols, terminal.rows);
      }
    });
    return () => window.cancelAnimationFrame(frame);
  }, [active, visible, sizing]);

  useEffect(() => {
    if ((sizing !== 'saved' && renderer !== 'dom') || !visible || restorePending) return;
    // Restores are parsed while hidden; repaint the DOM renderer once it becomes visible.
    const frame = window.requestAnimationFrame(() => {
      const terminal = terminalRef.current;
      if (terminal) terminal.refresh(0, terminal.rows - 1);
      reportFrame();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [sizing, renderer, visible, restorePending]);

  useEffect(() => {
    // The emulator is visibility:hidden during restore, so focusing it before
    // replay finishes has no effect. Retry once the active pane is ready.
    if ((!autoFocus && focusRequest === appliedFocusRequest.current) || !active || !visible || restorePending) return;
    const frame = window.requestAnimationFrame(() => {
      // Restore must not interrupt a menu or inline rename in either workspace's tab bar.
      if (inputSuspendedRef.current) return;
      if (document.activeElement?.closest('.project-aow-new, .project-aow-tab-rename-input, .project-aow-group-toggle')) return;
      appliedFocusRequest.current = focusRequest;
      terminalRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [active, visible, restorePending, autoFocus, focusRequest]);

  useEffect(() => {
    if (pane.status !== 'running') {
      setRestorePending(false);
      setConnection(pane.status);
    }
  }, [pane.status]);

  useEffect(() => {
    if (renderedIdentityRef.current === paneIdentity) return;
    renderedIdentityRef.current = paneIdentity;
    renderedStatusRef.current = undefined;
    setConnection(pane.status === 'running' ? 'connecting' : pane.status);
    setConnectionMessage(undefined);
    void terminalOperationQueueRef.current.barrier(() => {
      if (activeIdentityRef.current === paneIdentity) terminalRef.current?.reset();
    }).catch(() => {});
  }, [pane.status, paneIdentity]);

  useEffect(() => {
    if (!attachEnabled) {
      revokeInput();
      setRestorePending(false);
      setConnection(pane.status === 'running' ? 'disconnected' : pane.status);
      return;
    }
    type SocketContext = {
      socket: WebSocket;
      generation: number;
      messageQueue: Promise<void>;
      pendingMessages: Array<{ data: string | ArrayBuffer | Blob | Uint8Array; binaryBytes: number }>;
      drainingMessages: boolean;
      closed: Promise<void>;
      resolveClosed: () => void;
      allowReconnect: boolean;
      observerReattachStarted: boolean; // prevents duplicate reattach after a racing close event
      sawStream: boolean;
      legacyMode: boolean;
      replayBytesRemaining: number | undefined;
      replayDone: boolean;
      replayWriteStartedAt?: number;
      controlState: 'pending' | 'waiting' | 'observing' | 'claimed';
      forceClaim: boolean;
      mutationsRevoked: boolean;
      queuedBinaryBytes: number;
      inFlightBinaryBytes: number;
      globalQueuedBinaryBytes: number;
      backpressureClosing: boolean;
      requestSnapshotRecovery: boolean;
      snapshotRecoveryFallback?: { epoch: string; after: number };
      discardSubsequentMessages: boolean;
      waitingRetryTimer: number | undefined;
      waitingRetryAttempt: number;
      restoreDiagnostics?: TerminalRestoreDiagnostics;
    };

    let disposed = false;
    let activeContext: SocketContext | undefined;
    let retryTimer: number | undefined;
    let attempt = 0;
    let generation = 0;
    let restartEpoch = 0;
    let nextSnapshotRecoveryAt = 0;

    const contextIsActive = (context: SocketContext) => (
      !disposed
      && activeContext === context
      && activeIdentityRef.current === paneIdentity
      && context.generation === generation
    );

    const queueTerminalOperation = (
      context: SocketContext,
      operation: (terminal: Terminal) => void | Promise<void>,
    ) => terminalOperationQueueRef.current.barrier(() => {
      if (!contextIsActive(context)) return;
      const terminal = terminalRef.current;
      if (terminal) return operation(terminal);
    });

    const visibleHost = () => {
      const host = hostRef.current;
      return clipboardContextRef.current.visible && host !== null && host.clientWidth > 0 && host.clientHeight > 0
        ? host
        : undefined;
    };

    const sendResize = (context: SocketContext) => {
      if (!contextIsActive(context)
        || context.mutationsRevoked
        || context.controlState !== 'claimed'
        || context.socket !== inputReadySocketRef.current
        || context.socket.readyState !== WebSocket.OPEN) return;
      // A hidden activity is display:none. Never propagate xterm's fallback 2x1
      // geometry to the PTY while the terminal has no measurable viewport.
      if (!visibleHost()) return;
      const terminal = terminalRef.current;
      if (terminal) sendTerminalResizeNow(terminal.cols, terminal.rows);
    };

    const fitVisible = () => {
      if (presentationRef.current.sizing === 'saved') { reportFrame(); return false; }
      const host = visibleHost();
      const terminal = terminalRef.current;
      const fit = fitRef.current;
      if (!host || !terminal || !fit) return false;
      try { fitTerminal(terminal, fit, host); } catch { return false; }
      return true;
    };

    const markInputReady = async (context: SocketContext) => {
      await terminalOperationQueueRef.current.idle();
      if (!contextIsActive(context)) return;
      if (context.socket.readyState !== WebSocket.OPEN || context.backpressureClosing) return;
      if (context.mutationsRevoked || context.controlState !== 'claimed' || !context.sawStream || !context.replayDone) return;
      if (inputReadySocketRef.current === context.socket) return;
      attempt = 0;
      inputReadySocketRef.current = context.socket;
      setConnection('connected');
      setConnectionMessage(undefined);
      if (fitVisible()) {
        sendResize(context);
      }
      setRestorePending(false);
      const diagnostics = context.restoreDiagnostics;
      if (diagnostics && !diagnostics.reported) {
        diagnostics.reported = true;
        const readyAt = performance.now();
        const summary = {
          event: 'terminal_restore',
          paneId: pane.id,
          attachment: context.generation,
          mode: diagnostics.mode,
          websocketOpenMs: roundedMilliseconds(diagnostics.websocketOpenMs),
          handshakeMs: roundedMilliseconds(diagnostics.handshakeMs),
          snapshotUsed: diagnostics.snapshotUsed,
          snapshotBytes: diagnostics.snapshotBytes,
          snapshotWriteMs: roundedMilliseconds(diagnostics.snapshotWriteMs),
          replayBytes: diagnostics.replayBytes,
          replayWriteMs: roundedMilliseconds(diagnostics.replayWriteMs),
          readyMs: roundedMilliseconds(readyAt - diagnostics.startedAt),
        };
        console.debug('[terminal-restore]', summary);
        try {
          performance.measure('terminal_restore', {
            start: diagnostics.startedAt,
            end: readyAt,
            detail: summary,
          });
        } catch { /* Older browsers may not support measure options. */ }
      }
    };

    const markObserverReady = async (context: SocketContext) => {
      await terminalOperationQueueRef.current.idle();
      if (!contextIsActive(context)) return;
      if (context.socket.readyState !== WebSocket.OPEN || context.backpressureClosing) return;
      if (context.controlState !== 'observing' || !context.sawStream || !context.replayDone) return;
      attempt = 0;
      context.mutationsRevoked = true;
      revokeInput(context.socket);
      setConnection('observing');
      setConnectionMessage(undefined);
      setRestorePending(false);
    };

    const failProtocol = (context: SocketContext, message: string, closeReason: string) => {
      if (!contextIsActive(context)) return;
      context.allowReconnect = false;
      context.discardSubsequentMessages = true;
      context.mutationsRevoked = true;
      revokeInput(context.socket);
      setRestorePending(false);
      setConnection('disconnected');
      setConnectionMessage(message);
      context.socket.close(1000, closeReason);
    };

    const clearWaitingRetry = (context: SocketContext) => {
      if (context.waitingRetryTimer === undefined) return;
      window.clearTimeout(context.waitingRetryTimer);
      context.waitingRetryTimer = undefined;
    };

    const scheduleWaitingRetry = (context: SocketContext) => {
      if (!contextIsActive(context)
        || context.controlState !== 'waiting'
        || context.socket.readyState !== WebSocket.OPEN
        || context.waitingRetryTimer !== undefined) return;
      const delay = Math.min(
        WAITING_RETRY_MAX_MS,
        WAITING_RETRY_MIN_MS * 2 ** Math.min(context.waitingRetryAttempt, 4),
      );
      context.waitingRetryTimer = window.setTimeout(() => {
        context.waitingRetryTimer = undefined;
        if (!contextIsActive(context)
          || context.controlState !== 'waiting'
          || context.socket.readyState !== WebSocket.OPEN) return;
        context.waitingRetryAttempt += 1;
        context.socket.send(terminalApi.claimMessage(false));
      }, delay);
    };

    function reattachAsObserver(context: SocketContext) {
      if (!contextIsActive(context) || context.observerReattachStarted) return;
      // The daemon invalidates this controller before sending
      // attachment_superseded. Replace it immediately with a normal claim:
      // observer-aware daemons grant a read-only stream and older daemons
      // retain their existing waiting behavior.
      context.observerReattachStarted = true;
      context.allowReconnect = false;
      context.forceClaim = false;
      context.discardSubsequentMessages = true;
      context.mutationsRevoked = true;
      clearWaitingRetry(context);
      revokeInput(context.socket);
      clearPendingMessages(context);
      setRestorePending(false);
      setConnection('connecting');
      setConnectionMessage(t("terminals.another_window_took_control_switching_to_read_only_view"));
      context.socket.close(1000, 'terminal attachment superseded');
      // Do not depend on the peer's close timing: the old socket can no
      // longer mutate the PTY. Commit already submitted output before choosing
      // the resume offset, so a takeover cannot duplicate pending xterm writes.
      void terminalOperationQueueRef.current.idle().then(() => {
        if (contextIsActive(context)) connect(false);
      });
    }

    const renderStatus = async (
      context: SocketContext,
      status: Exclude<TerminalPaneStatus, 'running'>,
      exitCode?: number | null,
    ) => {
      context.allowReconnect = false;
      context.mutationsRevoked = true;
      revokeInput(context.socket);
      paneStatusRef.current = status;
      setConnection(status);
      const signature = `${status}:${exitCode ?? ''}`;
      setRestorePending(false);
      if (renderedStatusRef.current === signature) return;
      renderedStatusRef.current = signature;
      await queueTerminalOperation(context, (terminal) => terminalWriteln(terminal, status === 'exited'
        ? `\r\n[process exited${exitCode === undefined || exitCode === null ? '' : ` with code ${exitCode}`}]`
        : '\r\n[process interrupted]'));
      if (contextIsActive(context)) {
        callbackRef.current.onStatus(status, exitCode);
      }
    };

    const processMessage = async (context: SocketContext, data: string | ArrayBuffer | Blob | Uint8Array) => {
      if (!contextIsActive(context) || context.discardSubsequentMessages) {
        releaseQueuedBinaryBytes(context, binaryByteLength(data));
        return;
      }
      if (typeof data === 'string') {
        const control = controlMessage(data);
        if (control?.type === 'control') {
          if (control.state === undefined) {
            failProtocol(context, t("terminals.server_returned_invalid_terminal_control_state"), 'invalid terminal control state');
            return;
          }
          if (control.state === 'waiting') {
            if (context.controlState === 'claimed' || context.controlState === 'observing' || context.sawStream) {
              failProtocol(context, t("terminals.server_revoked_control_after_the_terminal_stream_started"), 'invalid terminal control transition');
              return;
            }
            context.controlState = 'waiting';
            context.forceClaim = false;
            context.mutationsRevoked = true;
            attempt = 0;
            revokeInput(context.socket);
            setRestorePending(false);
            setConnection('waiting');
            setConnectionMessage(undefined);
            scheduleWaitingRetry(context);
            return;
          }
          if (control.state === 'observing') {
            if (context.controlState === 'claimed' || context.sawStream) {
              failProtocol(context, t("terminals.server_changed_control_state_after_the_terminal_stream_started"), 'invalid terminal control transition');
              return;
            }
            context.controlState = 'observing';
            context.forceClaim = false;
            clearWaitingRetry(context);
            context.waitingRetryAttempt = 0;
            context.mutationsRevoked = true;
            attempt = 0;
            revokeInput(context.socket);
            setConnection('connecting');
            setConnectionMessage(undefined);
            return;
          }
          if (context.controlState === 'claimed') {
            if (context.legacyMode) {
              context.legacyMode = false;
              return;
            }
            failProtocol(context, t("terminals.server_granted_terminal_control_more_than_once"), 'duplicate terminal control claim');
            return;
          }
          context.controlState = 'claimed';
          context.forceClaim = false;
          clearWaitingRetry(context);
          context.waitingRetryAttempt = 0;
          context.mutationsRevoked = false;
          attempt = 0;
          setConnection('connecting');
          setConnectionMessage(undefined);
          return;
        }
        if (control?.type === 'stream') {
          if (context.controlState === 'pending') {
            // A v1 terminald sends Stream immediately and reports our Claim as
            // invalid_control. Treat that ordering as an implicitly claimed
            // legacy connection so rolling upgrades keep rendering output.
            context.legacyMode = true;
            context.controlState = 'claimed';
            context.forceClaim = false;
            context.mutationsRevoked = false;
          } else if (context.controlState !== 'claimed' && context.controlState !== 'observing') {
            failProtocol(context, t("terminals.server_sent_terminal_stream_before_granting_control"), 'terminal stream before control claim');
            return;
          }
          if (context.sawStream) {
            failProtocol(context, t("terminals.server_sent_terminal_stream_information_more_than_once"), 'duplicate terminal stream handshake');
            return;
          }
          if (control.epoch === undefined || control.offset === undefined || control.reset === undefined || control.replay_bytes === undefined) {
            failProtocol(context, t("terminals.server_returned_invalid_terminal_stream_information"), 'invalid terminal stream handshake');
            return;
          }
          const restoreValidation = terminalRestore(control as ControlMessage & { reset: boolean });
          if (!restoreValidation.ok) {
            failProtocol(context, restoreValidation.message, restoreValidation.closeReason);
            return;
          }
          if (context.snapshotRecoveryFallback) {
            const fallback = context.snapshotRecoveryFallback;
            const hasVtSnapshot = control.reset
              && restoreValidation.restore?.cols !== undefined
              && restoreValidation.restore.rows !== undefined;
            if (!hasVtSnapshot || control.replay_bytes > MAX_SNAPSHOT_RECOVERY_REPLAY_BYTES) {
              // A fresh attach without an actual VT snapshot would fall back
              // to replaying up to 8 MiB. Preserve the last frame xterm really
              // rendered and resume raw bytes from there instead.
              context.snapshotRecoveryFallback = undefined;
              context.discardSubsequentMessages = true;
              context.mutationsRevoked = true;
              revokeInput(context.socket);
              streamStateRef.current = {
                identity: paneIdentity,
                initialized: true,
                epoch: fallback.epoch,
                nextOffset: fallback.after,
              };
              setRestorePending(false);
              setConnection('disconnected');
              setConnectionMessage(t("terminals.terminal_snapshot_not_ready_resuming_from_the_rendered_position"));
              context.socket.close(1000, 'terminal snapshot recovery unavailable');
              return;
            }
            context.snapshotRecoveryFallback = undefined;
          }
          const previousStream = streamStateRef.current;
          const streamPositionMismatch = !control.reset
            && (previousStream.identity !== paneIdentity
              || !previousStream.initialized
              || control.epoch !== previousStream.epoch
              || control.offset !== previousStream.nextOffset);
          if (streamPositionMismatch) {
            context.mutationsRevoked = true;
            revokeInput(context.socket);
            streamStateRef.current = { identity: paneIdentity, initialized: false, epoch: '', nextOffset: 0 };
            setRestorePending(false);
            setConnection('disconnected');
            setConnectionMessage(t("terminals.terminal_resume_position_is_discontinuous_resyncing"));
            context.discardSubsequentMessages = true;
            context.socket.close(1000, 'terminal stream offset mismatch');
            return;
          }
          context.sawStream = true;
          context.replayBytesRemaining = control.replay_bytes;
          context.replayDone = control.replay_bytes === 0;
          const diagnostics = context.restoreDiagnostics;
          if (diagnostics) {
            const handshakeAt = performance.now();
            const hasVtSnapshot = control.reset
              && restoreValidation.restore?.cols !== undefined
              && restoreValidation.restore.rows !== undefined;
            diagnostics.handshakeMs = handshakeAt - (diagnostics.openedAt ?? diagnostics.startedAt);
            diagnostics.mode = control.reset
              ? (hasVtSnapshot ? 'vt_snapshot' : 'raw_replay')
              : 'resume';
            diagnostics.snapshotUsed = hasVtSnapshot;
            diagnostics.snapshotBytes = hasVtSnapshot && restoreValidation.restore
              ? (restoreValidation.restore.gzip?.decoded_bytes ?? utf8ByteLength(restoreValidation.restore.data))
              : 0;
            diagnostics.replayBytes = control.replay_bytes;
          }
          streamStateRef.current = { identity: paneIdentity, initialized: true, epoch: control.epoch, nextOffset: control.offset };
          if (control.reset) {
            setRestorePending(true);
            renderedStatusRef.current = undefined;
            await queueTerminalOperation(context, async (terminal) => {
              terminal.reset();
              // The serialized VT stream was produced at these exact
              // dimensions. Parse it at the same geometry so wrapped lines,
              // cursor positions and alternate-screen cells remain stable.
              // Input is still revoked here, so this local resize cannot be
              // reflected back to the PTY before replay completes.
              if (restoreValidation.restore?.cols !== undefined && restoreValidation.restore.rows !== undefined) {
                terminal.resize(restoreValidation.restore.cols, restoreValidation.restore.rows);
              }
              if (restoreValidation.restore !== undefined) {
                const snapshotWriteStartedAt = context.restoreDiagnostics ? performance.now() : 0;
                if (restoreValidation.restore.gzip) {
                  await writeCompressedTerminalSnapshot(restoreValidation.restore.gzip,
                    bytes => terminalWrite(terminal, bytes), () => contextIsActive(context) && !context.discardSubsequentMessages);
                } else await terminalWrite(terminal, restoreValidation.restore.data);
                if (context.restoreDiagnostics) {
                  context.restoreDiagnostics.snapshotWriteMs += performance.now() - snapshotWriteStartedAt;
                }
              }
            });
          }
          if (context.replayDone) {
            if (context.controlState === 'claimed') await markInputReady(context);
            else await markObserverReady(context);
          }
          return;
        }
        if (control?.type === 'status' && control.status) {
          if (context.controlState !== 'claimed' && context.controlState !== 'observing') {
            failProtocol(context, t("terminals.server_sent_terminal_state_before_granting_control"), 'terminal status before control claim');
            return;
          }
          if (control.status === 'running') {
            if (context.sawStream && context.replayDone) {
              if (context.controlState === 'claimed') await markInputReady(context);
              else await markObserverReady(context);
            }
          } else {
            await renderStatus(context, control.status, control.exit_code);
          }
          return;
        }
        if (control?.type === 'exit' || control?.type === 'exited') {
          if (context.controlState !== 'claimed' && context.controlState !== 'observing') {
            failProtocol(context, t("terminals.server_sent_terminal_state_before_granting_control"), 'terminal exit before control claim');
            return;
          }
          await renderStatus(context, 'exited', control.exit_code);
          return;
        }
        if (control?.type === 'error') {
          if (context.legacyMode && control.code === 'invalid_control') {
            return;
          }
          if (control.code === 'attachment_superseded') {
            reattachAsObserver(context);
          } else {
            setRestorePending(false);
            setConnectionMessage(control.message || t("terminals.terminal_connection_error"));
            if (control.code === 'output_lagged' || control.code === 'output_gap') {
              context.mutationsRevoked = true;
              revokeInput(context.socket);
              setConnection('disconnected');
              context.socket.close(1000, 'terminal output lagged');
            }
          }
          return;
        }
        if (!control) {
          failProtocol(context, t("terminals.server_returned_an_unrecognized_terminal_message"), 'unknown terminal text message');
        }
        return;
      }

      let submitted = false;
      try {
        const buffer = data instanceof Blob ? await data.arrayBuffer() : data;
        if (!contextIsActive(context) || context.discardSubsequentMessages) return;
        const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer);
        if ((context.controlState !== 'claimed' && context.controlState !== 'observing') || !context.sawStream) {
          failProtocol(context, t("terminals.server_sent_output_before_the_terminal_stream_was_ready"), 'terminal output before stream');
          return;
        }
        if (context.sawStream && !context.replayDone
          && (context.replayBytesRemaining === undefined || bytes.byteLength > context.replayBytesRemaining)) {
          context.discardSubsequentMessages = true;
          context.mutationsRevoked = true;
          revokeInput(context.socket);
          streamStateRef.current = { identity: paneIdentity, initialized: false, epoch: '', nextOffset: 0 };
          setRestorePending(false);
          setConnection('disconnected');
          setConnectionMessage(t("terminals.server_returned_invalid_terminal_replay_length_resyncing"));
          context.socket.close(1000, 'terminal replay length mismatch');
          return;
        }
        const replayChunk = !context.replayDone;
        if (replayChunk && context.replayBytesRemaining !== undefined) {
          context.replayBytesRemaining -= bytes.byteLength;
          context.replayWriteStartedAt ??= performance.now();
        }
        const committed = terminalOperationQueueRef.current.write(async () => {
          if (!contextIsActive(context) || context.discardSubsequentMessages) return;
          const terminal = terminalRef.current;
          if (!terminal) return;
          await terminalWrite(terminal, bytes);
          if (!contextIsActive(context)) return;
          const stream = streamStateRef.current;
          if (stream.identity === paneIdentity && stream.initialized) {
            const nextOffset = stream.nextOffset + bytes.byteLength;
            if (Number.isSafeInteger(nextOffset)) stream.nextOffset = nextOffset;
            else stream.initialized = false;
          }
        });
        submitted = true;
        context.inFlightBinaryBytes += bytes.byteLength;
        void committed.finally(() => {
          context.inFlightBinaryBytes -= bytes.byteLength;
          releaseQueuedBinaryBytes(context, bytes.byteLength);
        }).catch(() => failOutput(context));
        // Legacy replay may contain 64 MiB. Pipeline a bounded window so it
        // cannot overflow xterm's own write buffer while the socket queue drains.
        if (replayChunk && context.inFlightBinaryBytes >= MAX_LIVE_QUEUED_BINARY_BYTES) await committed;
        // Plain writes do not await their parse callback. Replay completion is
        // a barrier: input/resize and subsequent live output wait for its commit.
        if (replayChunk && context.replayBytesRemaining === 0) {
          await committed;
          if (!contextIsActive(context) || context.discardSubsequentMessages) return;
          if (context.restoreDiagnostics && context.replayWriteStartedAt !== undefined) {
            context.restoreDiagnostics.replayWriteMs = performance.now() - context.replayWriteStartedAt;
          }
          context.replayDone = true;
          if (context.controlState === 'claimed') await markInputReady(context);
          else await markObserverReady(context);
        }
      } finally {
        if (!submitted) releaseQueuedBinaryBytes(context, binaryByteLength(data));
      }
    };

    const binaryByteLength = (data: unknown) => (
      data instanceof ArrayBuffer || data instanceof Uint8Array
        ? data.byteLength
        : data instanceof Blob ? data.size : 0
    );

    const releaseQueuedBinaryBytes = (context: SocketContext, bytes: number) => {
      context.queuedBinaryBytes = Math.max(0, context.queuedBinaryBytes - bytes);
      const globallyQueued = Math.min(bytes, context.globalQueuedBinaryBytes);
      context.globalQueuedBinaryBytes -= globallyQueued;
      queuedTerminalBinaryBytes = Math.max(0, queuedTerminalBinaryBytes - globallyQueued);
    };

    const clearPendingMessages = (context: SocketContext) => {
      let binaryBytes = 0;
      for (const message of context.pendingMessages) binaryBytes += message.binaryBytes;
      context.pendingMessages.length = 0;
      releaseQueuedBinaryBytes(context, binaryBytes);
    };

    const closeForBackpressure = (context: SocketContext) => {
      if (!contextIsActive(context) || context.backpressureClosing) return true;
      const liveBacklogExceeded = context.replayDone
        && context.queuedBinaryBytes > MAX_LIVE_QUEUED_BINARY_BYTES;
      const totalBacklogExceeded = context.queuedBinaryBytes > MAX_REPLAY_QUEUED_BINARY_BYTES
        || queuedTerminalBinaryBytes > MAX_REPLAY_QUEUED_BINARY_BYTES;
      if (!liveBacklogExceeded && !totalBacklogExceeded) return false;

      // Stop accepting stale frames. After all xterm writes already in
      // flight complete, reconnect without a raw resume cursor so terminald
      // can restore the most recent VT snapshot plus its short raw suffix.
      context.backpressureClosing = true;
      context.requestSnapshotRecovery = true;
      context.discardSubsequentMessages = true;
      context.mutationsRevoked = true;
      revokeInput(context.socket);
      clearPendingMessages(context);
      setConnection('disconnected');
      setConnectionMessage(t("terminals.terminal_output_is_fast_syncing_the_latest_screen"));
      context.socket.close(1000, 'terminal output backpressure');
      return true;
    };

    const nextPendingMessage = (context: SocketContext) => {
      const first = context.pendingMessages.shift();
      if (!first || first.binaryBytes === 0 || first.data instanceof Blob) return first;

      const firstBinary = first.data as ArrayBuffer | Uint8Array;
      const chunks = [firstBinary instanceof Uint8Array ? firstBinary : new Uint8Array(firstBinary)];
      let binaryBytes = first.binaryBytes;
      // `replay_bytes` is a protocol barrier even though the next live frame
      // is also binary. Never merge across it: replay completion controls when
      // input and resize become legal again.
      const batchLimit = !context.replayDone && context.replayBytesRemaining !== undefined
        ? Math.min(MAX_XTERM_WRITE_BATCH_BYTES, context.replayBytesRemaining)
        : MAX_XTERM_WRITE_BATCH_BYTES;
      // Preserve rejection of frames that cross the replay/live boundary.
      if (!context.replayDone && context.replayBytesRemaining !== undefined
        && binaryBytes > context.replayBytesRemaining) return first;
      if (binaryBytes > batchLimit && batchLimit > 0) {
        context.pendingMessages.unshift({ data: chunks[0].subarray(batchLimit), binaryBytes: binaryBytes - batchLimit });
        return { data: chunks[0].subarray(0, batchLimit), binaryBytes: batchLimit };
      }
      while (context.pendingMessages.length > 0) {
        const next = context.pendingMessages[0];
        if (next.binaryBytes === 0 || next.data instanceof Blob
          || binaryBytes + next.binaryBytes > batchLimit) break;
        context.pendingMessages.shift();
        chunks.push(next.data instanceof Uint8Array ? next.data : new Uint8Array(next.data as ArrayBuffer));
        binaryBytes += next.binaryBytes;
      }
      if (chunks.length === 1) return first;
      const combined = new Uint8Array(binaryBytes);
      let offset = 0;
      for (const chunk of chunks) {
        combined.set(chunk, offset);
        offset += chunk.byteLength;
      }
      return { data: combined, binaryBytes };
    };

    const drainPendingMessages = (context: SocketContext) => {
      if (context.drainingMessages) return;
      context.drainingMessages = true;
      context.messageQueue = (async () => {
        try {
          while (contextIsActive(context) && !context.discardSubsequentMessages) {
            const message = nextPendingMessage(context);
            if (!message) break;
            await processMessage(context, message.data);
            if (closeForBackpressure(context)) break;
          }
        } finally {
          if (context.discardSubsequentMessages) clearPendingMessages(context);
          context.drainingMessages = false;
        }
      })().catch(() => failOutput(context));
    };

    function failOutput(context: SocketContext) {
      if (contextIsActive(context) && !context.backpressureClosing) {
        context.discardSubsequentMessages = true;
        context.mutationsRevoked = true;
        revokeInput(context.socket);
        clearPendingMessages(context);
        // Later pipelined writes may have parsed after a failed write. The
        // byte cursor is no longer proven contiguous; recover a full snapshot.
        streamStateRef.current = { identity: paneIdentity, initialized: false, epoch: '', nextOffset: 0 };
        setConnection('disconnected');
        setConnectionMessage(t("terminals.terminal_output_processing_failed_resuming"));
        context.socket.close(1000, 'terminal output processing failed');
      }
    }

    const reconnectAfterClose = async (context: SocketContext, epoch: number) => {
      await context.closed;
      await context.messageQueue;
      await terminalOperationQueueRef.current.idle();
      if (!contextIsActive(context) || epoch !== restartEpoch) return;
      if (!context.allowReconnect || paneStatusRef.current !== 'running') return;
      let snapshotRecoveryFallback: { epoch: string; after: number } | undefined;
      if (context.requestSnapshotRecovery && Date.now() >= nextSnapshotRecoveryAt) {
        const stream = streamStateRef.current;
        if (stream.identity === paneIdentity && stream.initialized) {
          snapshotRecoveryFallback = { epoch: stream.epoch, after: stream.nextOffset };
          nextSnapshotRecoveryAt = Date.now() + SNAPSHOT_RECOVERY_COOLDOWN_MS;
        }
      }
      context.mutationsRevoked = true;
      revokeInput(context.socket);
      setConnection('disconnected');
      attempt += 1;
      retryTimer = window.setTimeout(() => {
        retryTimer = undefined;
        if (contextIsActive(context) && epoch === restartEpoch) {
          connect(context.forceClaim, snapshotRecoveryFallback);
        }
      }, Math.min(8000, 500 * 2 ** Math.min(attempt - 1, 4)));
    };

    function connect(
      forceClaim = false,
      snapshotRecoveryFallback?: { epoch: string; after: number },
    ) {
      if (disposed) return;
      setConnection(attempt === 0 ? 'connecting' : 'disconnected');
      const stream = streamStateRef.current;
      const resume = snapshotRecoveryFallback === undefined
        && stream.identity === paneIdentity && stream.initialized
        ? { epoch: stream.epoch, after: stream.nextOffset }
        : undefined;
      const restoreDiagnostics: TerminalRestoreDiagnostics | undefined = terminalRestoreDebugEnabled()
        ? {
          startedAt: performance.now(),
          mode: resume ? 'resume' : 'raw_replay',
          snapshotUsed: false,
          snapshotBytes: 0,
          snapshotWriteMs: 0,
          replayBytes: 0,
          replayWriteMs: 0,
          reported: false,
        }
        : undefined;
      if (resume === undefined) setRestorePending(true);
      const currentSocket = new WebSocket(terminalApi.webSocketUrl(tabId, pane.id, resume));
      currentSocket.binaryType = 'arraybuffer';
      generation += 1;
      let resolveClosed = () => {};
      const closed = new Promise<void>((resolve) => { resolveClosed = resolve; });
      const context: SocketContext = {
        socket: currentSocket,
        generation,
        messageQueue: Promise.resolve(),
        pendingMessages: [],
        drainingMessages: false,
        closed,
        resolveClosed,
        allowReconnect: true,
        observerReattachStarted: false,
        sawStream: false,
        legacyMode: false,
        replayBytesRemaining: undefined,
        replayDone: false,
        controlState: 'pending',
        forceClaim,
        mutationsRevoked: false,
        queuedBinaryBytes: 0,
        inFlightBinaryBytes: 0,
        globalQueuedBinaryBytes: 0,
        backpressureClosing: false,
        requestSnapshotRecovery: false,
        snapshotRecoveryFallback,
        discardSubsequentMessages: false,
        waitingRetryTimer: undefined,
        waitingRetryAttempt: 0,
        restoreDiagnostics,
      };
      activeContext = context;
      socketRef.current = currentSocket;
      revokeInput();

      currentSocket.onopen = () => {
        if (!contextIsActive(context)) return;
        if (context.restoreDiagnostics) {
          context.restoreDiagnostics.openedAt = performance.now();
          context.restoreDiagnostics.websocketOpenMs = context.restoreDiagnostics.openedAt
            - context.restoreDiagnostics.startedAt;
        }
        setConnection('connecting');
        currentSocket.send(terminalApi.claimMessage(forceClaim));
      };
      currentSocket.onmessage = (event) => {
        if (!contextIsActive(context) || context.discardSubsequentMessages || context.backpressureClosing) return;
        if (typeof event.data === 'string') {
          const immediate = controlMessage(event.data);
          if (immediate?.type === 'error' && immediate.code === 'attachment_superseded') {
            // This must happen before queuing output behind a slow xterm write.
            reattachAsObserver(context);
            return;
          }
          if (immediate?.type === 'control' && (immediate.state === 'waiting' || immediate.state === 'observing')) {
            // Revoke synchronously rather than waiting behind xterm writes.
            context.mutationsRevoked = true;
            revokeInput(context.socket);
            if (immediate.type === 'control' && immediate.state === 'waiting') {
              context.controlState = 'waiting';
              context.forceClaim = false;
              setRestorePending(false);
              setConnection('waiting');
              setConnectionMessage(undefined);
              scheduleWaitingRetry(context);
            } else if (immediate.type === 'control') {
              context.controlState = 'observing';
              context.forceClaim = false;
              clearWaitingRetry(context);
              setConnection('connecting');
              setConnectionMessage(undefined);
            }
          }
        }
        const queuedBytes = binaryByteLength(event.data);
        context.queuedBinaryBytes += queuedBytes;
        context.globalQueuedBinaryBytes += queuedBytes;
        queuedTerminalBinaryBytes += queuedBytes;
        context.pendingMessages.push({
          data: event.data as string | ArrayBuffer | Blob,
          binaryBytes: queuedBytes,
        });
        if (!closeForBackpressure(context)) drainPendingMessages(context);
      };
      currentSocket.onerror = () => currentSocket.close();
      currentSocket.onclose = () => {
        clearWaitingRetry(context);
        context.mutationsRevoked = true;
        revokeInput(currentSocket);
        context.resolveClosed();
        if (currentSocket === socketRef.current) socketRef.current = undefined;
        if (currentSocket === inputReadySocketRef.current) inputReadySocketRef.current = undefined;
        void reconnectAfterClose(context, restartEpoch);
      };
    }

    manualReconnectRef.current = (force = false) => {
      if (force && agentPhaseRef.current && agentPhaseRef.current !== 'ready') return;
      restartEpoch += 1;
      if (retryTimer !== undefined) {
        window.clearTimeout(retryTimer);
        retryTimer = undefined;
      }
      setConnection('connecting');
      setConnectionMessage(undefined);
      const context = activeContext;
      if (!context) {
        connect(force);
        return;
      }
      context.allowReconnect = false;
      clearWaitingRetry(context);
      context.discardSubsequentMessages = true;
      context.mutationsRevoked = true;
      revokeInput(context.socket);
      clearPendingMessages(context);
      streamStateRef.current = { identity: paneIdentity, initialized: false, epoch: '', nextOffset: 0 };
      if (context.socket.readyState === WebSocket.CONNECTING || context.socket.readyState === WebSocket.OPEN) {
        context.socket.close(1000, 'manual terminal takeover');
      } else if (context.socket.readyState === WebSocket.CLOSED) {
        context.resolveClosed();
      }
      // A browser can keep a network-lost WebSocket OPEN or CLOSING for a long
      // time. Do not make an explicit user action wait for that stale socket's
      // close event. The terminal operation queue still keeps any write that
      // already started ahead of the new connection's reset/replay.
      connect(force);
    };

    connect(forceOnAttach);
    return () => {
      disposed = true;
      restartEpoch += 1;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
      manualReconnectRef.current = () => {};
      mutationGenerationRef.current += 1;
      inputReadySocketRef.current = undefined;
      const context = activeContext;
      if (context) clearWaitingRetry(context);
      if (context?.globalQueuedBinaryBytes) {
        queuedTerminalBinaryBytes = Math.max(0, queuedTerminalBinaryBytes - context.globalQueuedBinaryBytes);
        context.globalQueuedBinaryBytes = 0;
      }
      if (context) context.pendingMessages.length = 0;
      if (context?.socket === socketRef.current) socketRef.current = undefined;
      context?.socket.close(1000, 'terminal pane detached');
    };
  }, [pane.id, paneIdentity, tabId, attachEnabled]);

  return (
    <div
      ref={shellRef}
      className={`terminal-emulator-shell${pane.agent_terminal && sizing === 'saved' && !mobileInput ? ' terminal-cli-observer' : ''}${sizing === 'saved' ? ' terminal-saved-size' : ''}${restorePending ? ' restore-pending' : ''}`}
      data-terminal-sizing={sizing}
      aria-busy={restorePending}
      onPointerDown={onFocus}
      onDragEnter={dragExplorerPath}
      onDragOver={dragExplorerPath}
      onDragLeave={(event) => {
        if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setPathDropActive(false);
      }}
      onDrop={dropExplorerPath}
    >
      {visible && pendingLink?.identity === paneIdentity ? (
        <ConfirmationDialog
          title={t("terminals.open_link")}
          description={t("terminals.the_following_link_will_open_in_a_new_tab")}
          items={[{ id: pendingLink.url.href, label: pendingLink.url.host, detail: pendingLink.url.href, icon: <ExternalLink aria-hidden="true" /> }]}
          warning={t("terminals.confirm_you_trust_the_link_s_source_before_opening")}
          confirmLabel={t("terminals.open_link")}
          onResult={(confirmed) => {
            setPendingLink(null);
            // Open directly from the confirmation click to retain user activation.
            if (confirmed) window.open(pendingLink.url.href, '_blank', 'noopener,noreferrer');
          }}
        />
      ) : null}
      <div ref={hostRef} className="terminal-emulator" />
      {pathDropActive ? <div className="terminal-path-drop-overlay">{t("terminals.release_to_insert_path")}</div> : null}
      {restorePending ? (
        <div className="terminal-restore-status" role="status" aria-live="polite">
          <span aria-hidden="true" />
          {t("terminals.restoring_terminal")}</div>
      ) : null}
      {clipboardImageState ? (
        <div className={`terminal-clipboard-image-status ${clipboardImageState.status}`} role="status" aria-live="polite">
          <span aria-hidden="true" />
          {clipboardImageState.message}
        </div>
      ) : null}
      {clipboardCopyState && active && visible ? (
        <div className={`terminal-clipboard-copy-status ${clipboardCopyState.status}`} role="status" aria-live="polite">
          <ClipboardCopy aria-hidden="true" />
          <span>{clipboardCopyState.message}</span>
          {clipboardCopyState.text ? (
            <button type="button" onClick={(event) => { event.stopPropagation(); void completePendingClipboardCopy(); }}>{t("pr.copy")}</button>
          ) : null}
          <button
            type="button"
            className="dismiss"
            aria-label={t("terminals.dismiss_copy_prompt")}
            title={t("aow.close")}
            onClick={(event) => { event.stopPropagation(); clearClipboardCopyTimer(); setClipboardCopyState(null); }}
          >
            <X />
          </button>
        </div>
      ) : null}
      <div className={`terminal-connection ${connection}${connectionMessage ? ' has-message' : ''}${pane.hosting ? ' hosting' : ''}`} role="status" aria-live="polite">
        <span title={connectionMessage || pane.agent_terminal?.error || undefined}>{pane.hosting ? hostingStatus(pane.hosting) : terminalPaneStatusMessage(pane, connection, connectionMessage)}</span>
        {onRebuild && (pane.status === 'interrupted' || connection === 'interrupted') ? (
          <button
            type="button"
            className="terminal-rebuild-action"
            disabled={rebuilding}
            onClick={(event) => { event.stopPropagation(); onRebuild(); }}
          >
            {rebuilding ? t("terminals.rebuilding") : t("terminals.rebuild")}
          </button>
        ) : pane.hosting || connection === 'observing' || connection === 'waiting' ? (
          <button
            type="button"
            className="terminal-takeover-action"
            disabled={Boolean(pane.agent_terminal && pane.agent_terminal.phase !== 'ready')}
            onClick={(event) => { event.stopPropagation(); manualReconnectRef.current(true); }}
          >
            {t("acp-control.take_control")}</button>
        ) : connection === 'disconnected' ? (
          <button
            title={t("terminals.reconnect_now")}
            aria-label={t("terminals.reconnect_now")}
            onClick={(event) => { event.stopPropagation(); manualReconnectRef.current(false); }}
          >
            <RefreshCw />
          </button>
        ) : null}
        {pane.hosting && <TerminalHostingDetails hosting={pane.hosting} />}
      </div>
    </div>
  );
}
