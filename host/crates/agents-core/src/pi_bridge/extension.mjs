import { renameSync, rmSync, writeFileSync } from 'node:fs';

// Loaded explicitly by AoW. No persistent change to the user's Pi settings.
export default function aowPi(pi) {
  const binding = process.env.AOW_PI_BINDING;
  if (!binding) return;

  function publish(ctx) {
    const temporary = `${binding}.${process.pid}.tmp`;
    writeFileSync(temporary, JSON.stringify({
      pid: process.pid,
      cwd: ctx.sessionManager.getCwd(),
      session_id: ctx.sessionManager.getSessionId(),
      session_file: ctx.sessionManager.getSessionFile() ?? null,
    }), { mode: 0o600 });
    renameSync(temporary, binding);
  }

  function mark(ctx, event) {
    pi.appendEntry('aow.pi', { event, session_id: ctx.sessionManager.getSessionId() });
    publish(ctx);
  }

  pi.on('session_start', (_event, ctx) => mark(ctx, 'bind'));
  pi.on('session_tree', (_event, ctx) => mark(ctx, 'branch'));
  pi.on('agent_settled', (_event, ctx) => mark(ctx, 'settled'));
  pi.on('session_shutdown', () => rmSync(binding, { force: true }));
}
