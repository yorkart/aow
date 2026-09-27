import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

test('privileged registration treats requests as data and rolls back failed system registration', () => {
  const script = fileURLToPath(new URL('../register-launchdaemon.py', import.meta.url));
  const result = spawnSync('python3', ['-B', '-c', String.raw`
import copy, grp, importlib.util, json, os, pathlib, plistlib, pwd, stat, sys, tempfile
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('registration', sys.argv[1])
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

with tempfile.TemporaryDirectory(prefix='aow-registration-') as temporary:
    root = pathlib.Path(temporary)
    home = root / 'home'
    runtime = home / '.local/lib/aow'
    runtime.mkdir(parents=True, mode=0o700)
    for path in [home, home / '.local', home / '.local/lib']:
        path.chmod(0o700)
    account = SimpleNamespace(pw_name='aow-test', pw_uid=max(501, os.getuid()), pw_gid=os.getgid(), pw_dir=str(home))
    def config(component, port='8282'):
        return {
            'Label': f'org.aow.service.{component}', 'UserName': account.pw_name,
            'GroupName': grp.getgrgid(account.pw_gid).gr_name,
            'ProgramArguments': [str(runtime / 'bin' / f'aow-{component}')],
            'WorkingDirectory': str(home), 'RunAtLoad': True, 'KeepAlive': True,
            'ThrottleInterval': 1, 'Umask': 63,
            'StandardOutPath': '/dev/null', 'StandardErrorPath': '/dev/null',
            'EnvironmentVariables': {'HOME': str(home), 'USER': account.pw_name,
                'LOGNAME': account.pw_name, 'AOW_RUNTIME_ROOT': str(runtime),
                'AOW_LOG_MODE': 'unified', 'AOW_SERVER_PORT': port},
        }
    request = {'version': 1, 'user': account.pw_name, 'uid': account.pw_uid,
               'home': str(home), 'runtime': str(runtime),
               'components': {name: config(name) for name in ['server', 'terminald']}}
    valid = m.validate_request(request, account)
    assert [name for name, _ in valid] == ['terminald', 'server']

    def rejects(document, expected_account=account):
        try:
            m.validate_request(document, expected_account)
        except (ValueError, KeyError):
            return
        raise AssertionError('malicious request was accepted')
    for key, value in [('UserName', 'root'), ('GroupName', 'wrong-group'),
                       ('ProgramArguments', ['/bin/sh', '-c', 'touch /etc/forbidden']),
                       ('StandardOutPath', '/etc/sudoers'), ('WorkingDirectory', '/'),
                       ('KeepAlive', False), ('Umask', 0), ('UserName', None)]:
        bad = copy.deepcopy(request)
        bad['components']['server'][key] = value
        rejects(bad)
    for key in ['Sockets', 'MachServices', 'Program', 'RootDirectory']:
        bad = copy.deepcopy(request)
        bad['components']['server'][key] = {}
        rejects(bad)
    for key, value in [('HOME', '/var/root'), ('USER', 'root'), ('AOW_RUNTIME_ROOT', '/tmp/other'),
                       ('AOW_LOG_MODE', 'file'), ('DYLD_INSERT_LIBRARIES', '/tmp/evil'),
                       ('PATH', '/usr/bin\nmalformed')]:
        bad = copy.deepcopy(request)
        bad['components']['server']['EnvironmentVariables'][key] = value
        rejects(bad)
    bad = copy.deepcopy(request)
    bad['user'] = 'another-user'
    rejects(bad)
    rejects(request, SimpleNamespace(**{**account.__dict__, 'pw_uid': 0}))
    bad = copy.deepcopy(request)
    bad['components'] = {'arbitrary': {}}
    rejects(bad)

    # The reader checks ownership, privacy, symlinks and bounded input without
    # changing any real account or accessing /Library/LaunchDaemons.
    reader_account = SimpleNamespace(**{**account.__dict__, 'pw_uid': os.getuid()})
    request_path = runtime / 'pending-launchdaemon.json'
    request_path.write_text(json.dumps(request))
    request_path.chmod(0o600)
    assert m.read_request(reader_account) == request
    def reader_rejects():
        try:
            m.read_request(reader_account)
        except (ValueError, OSError):
            return
        raise AssertionError('unsafe request file accepted')
    request_path.chmod(0o644)
    reader_rejects()
    request_path.unlink()
    payload = root / 'payload'
    payload.write_text(json.dumps(request))
    payload.chmod(0o600)
    request_path.symlink_to(payload)
    reader_rejects()
    request_path.unlink()
    os.mkfifo(request_path, 0o600)
    reader_rejects()  # Must reject promptly, even when nobody has opened a writer.
    request_path.unlink()
    request_path.write_text('x' * (1024 * 1024 + 1))
    request_path.chmod(0o600)
    reader_rejects()
    runtime.rename(runtime.with_name('saved-runtime'))
    runtime.symlink_to(runtime.with_name('saved-runtime'), target_is_directory=True)
    reader_rejects()

    daemons = root / 'LaunchDaemons'
    daemons.mkdir()
    m.DAEMON_DIRECTORY = daemons
    labels = {f'org.aow.service.{name}' for name in ['server', 'terminald']}
    running = set(labels)
    previous = {}
    for component in ['server', 'terminald']:
        path = daemons / f'org.aow.service.{component}.plist'
        previous[path] = plistlib.dumps(config(component))
        path.write_bytes(previous[path])
    original_lstat = pathlib.Path.lstat
    def root_owned(path):
        values = list(original_lstat(path))
        values[stat.ST_UID] = 0
        return os.stat_result(values)
    fail_once = [True]
    calls = []
    def mocked_launchctl(*args):
        calls.append(args)
        if args[0] == 'print-disabled':
            return '{}'
        if args[0] == 'bootout':
            running.discard(args[1].split('/')[-1])
        elif args[0] == 'bootstrap':
            label = pathlib.Path(args[2]).stem
            document = plistlib.loads(pathlib.Path(args[2]).read_bytes())
            assert document['UserName'] == account.pw_name
            if label.endswith('.server') and fail_once[0]:
                fail_once[0] = False
                raise RuntimeError('simulated bootstrap failure')
            running.add(label)
        return ''
    desired = [(name, config(name, '8283')) for name in ['terminald', 'server']]
    with patch.object(pathlib.Path, 'lstat', root_owned), \
         patch.object(m, 'launchctl', mocked_launchctl), \
         patch.object(m, 'loaded', lambda label: label in running), \
         patch.object(m, 'write_plist', lambda path, content: path.write_bytes(content)):
        try:
            m.register(desired, account)
        except RuntimeError as error:
            assert 'simulated bootstrap failure' in str(error)
        else:
            raise AssertionError('expected registration to fail')
        assert running == labels
        assert all(path.read_bytes() == content for path, content in previous.items())
        m.register(desired, account)
        assert running == labels
        assert all(plistlib.loads(path.read_bytes())['EnvironmentVariables']['AOW_SERVER_PORT'] == '8283'
                   for path in previous)
    print('registration validation, file safety and rollback passed')
`, script], { encoding: 'utf8', timeout: 10000 });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
