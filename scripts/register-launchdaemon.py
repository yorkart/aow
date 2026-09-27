#!/usr/bin/env python3
"""Register prepared AoW daemons from an administrator's local terminal.

Run a trusted, administrator-owned copy with: sudo -k /usr/bin/python3 -I ...
The service account's request is data. Never execute its scripts as root.
"""
import argparse
import grp
import json
import os
from pathlib import Path
import plistlib
import pwd
import re
import stat
import subprocess
import sys
import tempfile
import time

DAEMON_DIRECTORY = Path('/Library/LaunchDaemons')
LAUNCHCTL = '/bin/launchctl'
ENVIRONMENT_KEYS = {
    'HOME', 'USER', 'LOGNAME', 'SHELL', 'PATH', 'LANG', 'LC_ALL', 'LC_CTYPE',
    'AOW_RUNTIME_ROOT', 'AOW_SERVER_HOST', 'AOW_SERVER_PORT', 'AOW_SERVER_STATE_DIR',
    'AOW_STATE_DIR', 'AOW_TERMINALD_SOCKET', 'AOW_BASE_PATH', 'XDG_STATE_HOME',
    'XDG_RUNTIME_DIR', 'AOW_LOG_MODE',
}


def read_request(account):
    """Open each user-controlled path component without following symlinks."""
    descriptor = os.open(account.pw_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in ('.local', 'lib', 'aow'):
            info = os.fstat(descriptor)
            if info.st_uid not in (0, account.pw_uid) or info.st_mode & 0o022:
                raise ValueError('Request directory has unsafe ownership or permissions')
            following = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = following
        info = os.fstat(descriptor)
        if info.st_uid != account.pw_uid or info.st_mode & 0o022:
            raise ValueError('Runtime directory must belong to the service account and not be group/world writable')
        request_fd = os.open('pending-launchdaemon.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=descriptor)
        with os.fdopen(request_fd, 'rb') as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_uid != account.pw_uid or info.st_mode & 0o077:
                raise ValueError('Request must be a private regular file owned by the service account')
            content = stream.read(1024 * 1024 + 1)
            if len(content) > 1024 * 1024:
                raise ValueError('Request exceeds size limit')
            return json.loads(content)
    finally:
        os.close(descriptor)


def validate_config(config, component, account):
    """Reconstruct an allowlisted plist; reject all extra launchd capabilities."""
    service_home = account.pw_dir
    runtime = str(Path(service_home) / '.local/lib/aow')
    expected = {
        'Label': f'org.aow.service.{component}',
        'UserName': account.pw_name,
        'GroupName': grp.getgrgid(account.pw_gid).gr_name,
        'ProgramArguments': [str(Path(runtime) / 'bin' / f'aow-{component}')],
        'WorkingDirectory': service_home,
        'RunAtLoad': True,
        'KeepAlive': True,
        'ThrottleInterval': 1,
        'Umask': 0o077,
        'StandardOutPath': '/dev/null',
        'StandardErrorPath': '/dev/null',
    }
    if not isinstance(config, dict) or set(config) != set(expected) | {'EnvironmentVariables'}:
        raise ValueError('Unexpected LaunchDaemon keys')
    for key, value in expected.items():
        if type(config[key]) is not type(value) or config[key] != value:
            raise ValueError(f'Invalid LaunchDaemon field: {key}')
    environment = config['EnvironmentVariables']
    if not isinstance(environment, dict) or not set(environment) <= ENVIRONMENT_KEYS:
        raise ValueError('Unexpected LaunchDaemon environment keys')
    for value in environment.values():
        if not isinstance(value, str) or len(value) > 16384 or any(ord(c) < 32 for c in value):
            raise ValueError('Invalid LaunchDaemon environment value')
    for key, value in {'HOME': service_home, 'USER': account.pw_name, 'LOGNAME': account.pw_name,
                       'AOW_RUNTIME_ROOT': runtime, 'AOW_LOG_MODE': 'unified'}.items():
        if environment.get(key) != value:
            raise ValueError(f'Invalid service account environment: {key}')
    expected['EnvironmentVariables'] = dict(environment)
    return expected


def validate_request(document, account):
    if account.pw_uid == 0:
        raise ValueError('AoW must run as a non-root account')
    if not isinstance(document, dict) or document.get('version') != 1:
        raise ValueError('Invalid request version')
    for key, value in {'user': account.pw_name, 'uid': account.pw_uid, 'home': account.pw_dir,
                       'runtime': str(Path(account.pw_dir) / '.local/lib/aow')}.items():
        if document.get(key) != value:
            raise ValueError(f'Request does not match the target account: {key}')
    components = document.get('components')
    if not isinstance(components, dict) or not components or not set(components) <= {'server', 'terminald'}:
        raise ValueError('Invalid requested components')
    # Start terminald first if both components were explicitly prepared.
    return [(component, validate_config(components[component], component, account))
            for component in ('terminald', 'server') if component in components]


def launchctl(*arguments):
    result = subprocess.run([LAUNCHCTL, *arguments], capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise RuntimeError(f'launchctl {arguments[0]} failed: {result.stderr.strip()}')
    return result.stdout


def loaded(label):
    result = subprocess.run([LAUNCHCTL, 'print', f'system/{label}'], capture_output=True, text=True, timeout=10)
    if result.returncode == 0:
        return True
    if result.returncode == 113 and re.search(r'could not find (?:domain|service)', result.stderr, re.I):
        return False
    raise RuntimeError('Unable to inspect system LaunchDaemon state')


def unload(label):
    if not loaded(label):
        return
    launchctl('bootout', f'system/{label}')
    deadline = time.monotonic() + 30
    while loaded(label):
        if time.monotonic() >= deadline:
            raise RuntimeError(f'Timed out removing {label}')
        time.sleep(0.1)


def write_plist(path, content):
    descriptor, temporary = tempfile.mkstemp(prefix='.aow-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
            os.fchown(stream.fileno(), 0, 0)
            os.fchmod(stream.fileno(), 0o644)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def register(configurations, account):
    info = DAEMON_DIRECTORY.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
        raise ValueError('Untrusted LaunchDaemons directory')
    disabled = launchctl('print-disabled', 'system')
    snapshots = []
    for component, config in configurations:
        label = config['Label']
        path = DAEMON_DIRECTORY / f'{label}.plist'
        previous = None
        try:
            info = path.lstat()
            if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
                raise ValueError(f'Untrusted existing plist: {path}')
            previous = path.read_bytes()
            validate_config(plistlib.loads(previous), component, account)
        except FileNotFoundError:
            pass
        was_loaded = loaded(label)
        if was_loaded and previous is None:
            raise ValueError(f'Refusing to replace an unmanaged loaded job: {label}')
        was_disabled = bool(re.search(r'"' + re.escape(label) + r'"\s*=>\s*true', disabled))
        snapshots.append((path, config, previous, was_loaded, was_disabled))
    changed = []
    try:
        for snapshot in snapshots:
            path, config, previous, was_loaded, was_disabled = snapshot
            changed.append(snapshot)
            label = config['Label']
            unload(label)
            write_plist(path, plistlib.dumps(config, sort_keys=False))
            launchctl('enable', f'system/{label}')
            launchctl('bootstrap', 'system', str(path))
            print(f'Registered {label} as {account.pw_name}', flush=True)
    except BaseException:
        for path, config, previous, was_loaded, was_disabled in reversed(changed):
            try:
                label = config['Label']
                unload(label)
                if previous is None:
                    if path.exists():
                        path.unlink()
                else:
                    write_plist(path, previous)
                launchctl('disable' if was_disabled else 'enable', f'system/{label}')
                if was_loaded:
                    launchctl('bootstrap', 'system', str(path))
            except Exception as error:
                print(f'Rollback requires administrator attention for {config["Label"]}: {error}', file=sys.stderr)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--user', required=True, help='Existing non-root service account')
    args = parser.parse_args()
    if sys.platform != 'darwin' or os.geteuid() != 0:
        parser.error('Run a trusted copy with sudo on macOS, from an administrator local terminal')
    if not sys.stdin.isatty() or not sys.stderr.isatty() or os.environ.get('SSH_CONNECTION') or os.environ.get('SSH_TTY'):
        parser.error('Registration requires an interactive local administrator terminal, not SSH')
    account = pwd.getpwnam(args.user)
    if account.pw_uid == 0:
        parser.error('Refusing to register AoW as root')
    configurations = validate_request(read_request(account), account)
    print(f'Registering {len(configurations)} prepared service(s) for {account.pw_name}. Existing sessions in these jobs will end.', flush=True)
    register(configurations, account)
    print('Registration completed. Rerun just install or aow update as the service account to verify activation.')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print(f'error: {error}', file=sys.stderr)
        sys.exit(1)
