import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

test('just install forwards its optional user and keeps positional package compatibility', t => {
  const directory = mkdtempSync(join(tmpdir(), 'aow-install-entry-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  mkdirSync(join(directory, 'scripts'));
  mkdirSync(join(directory, 'tools'));
  copyFileSync(new URL('../../justfile', import.meta.url), join(directory, 'justfile'));
  copyFileSync(new URL('../install-local.sh', import.meta.url), join(directory, 'scripts/install-local.sh'));
  writeFileSync(join(directory, 'tools/uname'), '#!/bin/sh\necho Darwin\n', { mode: 0o755 });
  writeFileSync(join(directory, 'scripts/install-macos.py'), 'import json,sys\nprint(json.dumps(sys.argv[1:]))\n');
  for (const [args, expected] of [
    [[], ['--package', 'target/packages/latest']],
    [['--user', 'aow-service'], ['--package', 'target/packages/latest', '--user', 'aow-service']],
    [['a package.tar.gz', '--user', 'aow-service'], ['--package', 'a package.tar.gz', '--user', 'aow-service']],
    [['--user', 'aow-service', '--package', 'a package.tar.gz'], ['--package', 'a package.tar.gz', '--user', 'aow-service']],
  ]) {
    const result = spawnSync('just', ['install', ...args], { cwd: directory, encoding: 'utf8', timeout: 10000,
      env: { ...process.env, PATH: `${join(directory, 'tools')}:${process.env.PATH}` } });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), expected);
  }
});

test('macOS account selection, privilege separation and administrator installation', () => {
  const script = fileURLToPath(new URL('../install-macos.py', import.meta.url));
  const result = spawnSync('python3', ['-B', '-c', String.raw`
import contextlib, importlib.util, io, json, os, pathlib, stat, sys, tempfile
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('installer', sys.argv[1])
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
caller = SimpleNamespace(pw_name='operator', pw_uid=501, pw_gid=20, pw_dir='/Users/operator', pw_shell='/bin/zsh')
target = SimpleNamespace(pw_name='aow-service', pw_uid=502, pw_gid=20, pw_dir='/Users/aow-service', pw_shell='/bin/zsh')
root = SimpleNamespace(pw_name='root', pw_uid=0, pw_gid=0, pw_dir='/var/root', pw_shell='/bin/sh')

def entry(arguments, admin=False, registration=False, uid=501):
    calls = []
    def run(args, **kwargs):
        calls.append((args, kwargs))
        return SimpleNamespace(returncode=0, stdout=json.dumps({'registrationRequired': registration}), stderr='')
    with patch.object(m.sys, 'platform', 'darwin'), patch.object(m.os, 'getuid', return_value=uid), \
         patch.object(m.pwd, 'getpwuid', return_value=caller if uid else root), \
         patch.object(m.pwd, 'getpwnam', side_effect=lambda name: {'operator': caller, 'aow-service': target, 'root': root}[name]), \
         patch.object(m, 'is_admin', return_value=admin), patch.object(m, 'require_local_terminal'), \
         patch.object(m, 'trusted_path', side_effect=lambda path, owner: pathlib.Path(path).absolute()), \
         patch.object(m.subprocess, 'run', side_effect=run), contextlib.redirect_stdout(io.StringIO()), \
         contextlib.redirect_stderr(io.StringIO()):
        status = m.main(['--package', 'local package.tar.gz', *arguments])
    return status, calls

# Default and explicit-current-account updates never elevate. GUI first installs
# use the same path. Another target requires the caller's administrator rights.
for args in [[], ['--user', 'operator']]:
    status, calls = entry(args)
    assert status == 0 and len(calls) == 2
    assert calls[0][0][-1] == '/Users/operator/.local/lib/aow'
    assert calls[1][0][0] == '/bin/bash'
    assert not any('/usr/bin/sudo' in command for command, _ in calls)
status, calls = entry([], registration=True)
assert status == 78 and len(calls) == 1
status, calls = entry(['--user', 'aow-service'])
assert status == 78 and calls == []
for args, expected in [([], 'operator'), (['--user', 'aow-service'], 'aow-service')]:
    status, calls = entry(args, admin=True, registration=True)
    command = calls[-1][0]
    assert status == 0 and command[:4] == ['/usr/bin/sudo', '-k', '/usr/bin/python3', '-I']
    assert command[command.index('--user') + 1] == expected
for args, uid in [(['--user', 'root'], 501), ([], 0)]:
    try:
        entry(args, uid=uid)
    except SystemExit as error:
        assert error.code != 0
    else:
        raise AssertionError('root installation was accepted')

with patch.dict(os.environ, {'SSH_AUTH_SOCK': '/private/admin-agent', 'PYTHONPATH': '/private/admin-code',
                             'AOW_SERVER_STATE_DIR': '/Users/operator/private-data'}, clear=True):
    env = m.target_environment(target, True)
assert env['HOME'] == target.pw_dir and env['USER'] == target.pw_name
assert env['AOW_INSTALL_TERMINALD'] == 'y'
assert not {'SSH_AUTH_SOCK', 'PYTHONPATH', 'AOW_SERVER_STATE_DIR'} & env.keys()
with patch.object(m.os, 'getgrouplist', return_value=[20, 12]), patch.object(m.subprocess, 'run') as run:
    run.return_value = SimpleNamespace(returncode=0, stdout='shell greeting\nAOW_INSTALL_PATH=/Users/aow-service/.nvm/node/bin:/usr/bin\n')
    assert m.target_login_path(target, env).startswith('/Users/aow-service/.nvm/')
    assert run.call_args.kwargs['user'] == target.pw_uid
    assert run.call_args.kwargs['env']['HOME'] == target.pw_dir
    run.return_value = SimpleNamespace(returncode=1, stdout='')
    assert m.target_login_path(target, env) == env['PATH']
with patch.object(m.os, 'getgrouplist', return_value=[20, 12]), patch.object(m.subprocess, 'run') as run:
    run.return_value.returncode = 0
    assert m.run_as_user(target, ['/bin/bash', '/private/tmp/installer'], env) == 0
    options = run.call_args.kwargs
    assert (options['user'], options['group'], options['extra_groups']) == (502, 20, [20, 12])
    assert options['cwd'] == target.pw_dir and options['env'] == env

# Administrator inputs stay immutable while scripts and verification run as the
# target. Only validated, currently confirmed components reach registration.
with tempfile.TemporaryDirectory(prefix='aow-admin-test-') as temporary:
    directory = pathlib.Path(temporary)
    installer = directory / 'install-release.sh'
    installer.write_text('#!/bin/sh\nexit 78\n')
    archive = directory / 'aow.tar.gz'
    archive.write_bytes(b'package data')
    pathlib.Path(str(archive) + '.sha256').write_text('checksum data')
    registration = directory / 'register-launchdaemon.py'
    registration.write_text('# trusted registration helper\n')
    for reply, components in [('y', ['terminald', 'server']), ('n', ['server']), (' y', ['server'])]:
        events = []
        frozen_paths = []
        def run_target(account, arguments, environment):
            assert account is target
            if arguments[0] == '/bin/bash':
                frozen = pathlib.Path(arguments[-1])
                frozen_paths.append(frozen)
                assert frozen.read_bytes() == b'package data'
                assert stat.S_IMODE(frozen.parent.stat().st_mode) == 0o711
                assert stat.S_IMODE(frozen.stat().st_mode) == 0o644
                assert environment['AOW_INSTALL_TERMINALD'] == ('y' if reply == 'y' else 'n')
                events.append('prepare as target')
                return 78
            events.append('verify ' + arguments[-2])
            assert arguments[:2] == ['/usr/bin/env', 'node']
            assert arguments[2].startswith(target.pw_dir + '/.local/lib/aow/')
            return 0
        helper = SimpleNamespace(read_request=lambda account: {'components': {'server': {}, 'terminald': {}}},
            validate_request=lambda request, account: [('terminald', {}), ('server', {})],
            register=lambda configs, account: events.append('register ' + ','.join(name for name, _ in configs)))
        with patch.object(m, 'SCRIPT_DIR', directory), patch.object(m, 'require_local_terminal'), \
             patch.object(m, 'target_login_path', return_value='/usr/bin:/bin'), \
             patch.object(m, 'trusted_path', side_effect=lambda path, owner: pathlib.Path(path)), \
             patch('builtins.input', return_value=reply), patch.object(m, 'run_as_user', side_effect=run_target), \
             patch.object(m, 'registration_module', return_value=helper), contextlib.redirect_stdout(io.StringIO()):
            assert m.install_as_administrator(target, archive, caller.pw_uid) == 0
        assert events == ['prepare as target', 'register ' + ','.join(components), *['verify ' + name for name in components]]
        assert all(not path.exists() for path in frozen_paths)
    # Errors stop the coordinator rather than registering partial installation.
    for code in [1, 130]:
        with patch.object(m, 'SCRIPT_DIR', directory), patch.object(m, 'require_local_terminal'), \
             patch.object(m, 'target_login_path', return_value='/usr/bin:/bin'), \
             patch.object(m, 'trusted_path', side_effect=lambda path, owner: pathlib.Path(path)), \
             patch('builtins.input', return_value='n'), patch.object(m, 'run_as_user', return_value=code), \
             patch.object(m, 'registration_module') as load_helper, contextlib.redirect_stdout(io.StringIO()):
            assert m.install_as_administrator(target, archive, caller.pw_uid) == code
            load_helper.assert_not_called()
    archive.chmod(0o666)
    try:
        m.trusted_path(archive, os.getuid())
    except ValueError:
        pass
    else:
        raise AssertionError('writable administrator input was accepted')

print('account selection, privilege separation, consent and installation sequencing passed')
`, script], { encoding: 'utf8', timeout: 15000 });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
