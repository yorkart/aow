#!/usr/bin/env python3
"""Install for a macOS account, elevating only the administrator coordinator.

All package scripts, binaries, account setup and health checks run as the target
account. Only the trusted registration helper runs with root privileges.
"""
import argparse
import grp
import json
import os
from pathlib import Path
import pwd
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
from types import ModuleType

SCRIPT_DIR = Path(__file__).resolve().parent


def is_admin(account):
    return grp.getgrnam('admin').gr_gid in os.getgrouplist(account.pw_name, account.pw_gid)


def require_local_terminal():
    if not sys.stdin.isatty() or not sys.stderr.isatty() or os.environ.get('SSH_CONNECTION') or os.environ.get('SSH_TTY'):
        raise ValueError('Administrator installation requires an interactive local terminal on this Mac, not SSH')


def trusted_path(path, administrator_uid):
    """Root only reads code and packages controlled by the invoking admin."""
    path = Path(path).resolve(strict=True)
    if not path.is_file():
        raise ValueError(f'Expected a regular file: {path}')
    for entry in (path, *path.parents):
        info = entry.stat()
        sticky_root_directory = stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and info.st_mode & stat.S_ISVTX
        if info.st_uid not in (0, administrator_uid) or (info.st_mode & 0o022 and not sticky_root_directory):
            raise ValueError(f'Use an administrator-owned checkout and package, not writable by other accounts: {entry}')
    return path


def target_environment(account, terminald):
    home = account.pw_dir
    return {
        'HOME': home, 'USER': account.pw_name, 'LOGNAME': account.pw_name,
        'SHELL': account.pw_shell or '/bin/zsh', 'LANG': 'en_US.UTF-8',
        'PATH': ':'.join([f'{home}/.local/bin', f'{home}/.local/lib/aow', f'{home}/.cargo/bin',
                          '/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin']),
        'TMPDIR': '/private/tmp', 'AOW_INSTALL_MANAGED': '1',
        'AOW_INSTALL_TERMINALD': 'y' if terminald else 'n',
    }


def user_process_options(account, environment):
    return dict(cwd=account.pw_dir, env=environment, user=account.pw_uid,
                group=account.pw_gid, extra_groups=os.getgrouplist(account.pw_name, account.pw_gid))


def target_login_path(account, environment):
    # nvm and similar managers usually initialize PATH in the user's shell rc.
    # Load that shell only after dropping privileges; retain PATH, not credentials
    # or HOME/config overrides from either the administrator or the shell.
    result = subprocess.run([account.pw_shell or '/bin/zsh', '-ilc',
                             'printf "\\nAOW_INSTALL_PATH=%s\\n" "$PATH"'],
                            stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30,
                            **user_process_options(account, environment))
    paths = [line.removeprefix('AOW_INSTALL_PATH=') for line in result.stdout.splitlines()
             if line.startswith('AOW_INSTALL_PATH=')]
    if result.returncode == 0 and paths and paths[-1]:
        return paths[-1]
    return environment['PATH']


def run_as_user(account, arguments, environment):
    return subprocess.run(arguments, **user_process_options(account, environment)).returncode


def registration_module(path):
    # Execute the validated source, never an adjacent bytecode cache.
    module = ModuleType('aow_registration')
    module.__file__ = str(path)
    exec(compile(path.read_bytes(), str(path), 'exec'), module.__dict__)
    return module


def install_as_administrator(account, package, administrator_uid):
    require_local_terminal()
    trusted_path(__file__, administrator_uid)
    installer = trusted_path(SCRIPT_DIR / 'install-release.sh', administrator_uid)
    registration = trusted_path(SCRIPT_DIR / 'register-launchdaemon.py', administrator_uid)
    archive = trusted_path(package, administrator_uid)
    checksum = trusted_path(str(archive) + '.sha256', administrator_uid)
    print(f'Installation account: {account.pw_name} (HOME={account.pw_dir})', flush=True)
    answer = input('Start/restart terminald for this account? This ends its existing terminal sessions. Enter lowercase y to confirm: ')
    terminald = answer == 'y'
    environment = target_environment(account, terminald)
    environment['PATH'] = target_login_path(account, environment)
    with tempfile.TemporaryDirectory(prefix='aow-admin-install-', dir='/private/tmp') as temporary:
        stage = Path(temporary)
        # The target can read the frozen inputs but cannot replace them.
        stage.chmod(0o711)
        for source, name in [(installer, 'install-release.sh'), (archive, archive.name),
                             (checksum, archive.name + '.sha256')]:
            destination = stage / name
            shutil.copyfile(source, destination)
            destination.chmod(0o644)
        result = run_as_user(account, ['/bin/bash', str(stage / 'install-release.sh'),
                                       '--package', str(stage / archive.name)], environment)
        if result != 78:
            return result
        helper = registration_module(registration)
        request = helper.read_request(account)
        configurations = helper.validate_request(request, account)
        # A previous interrupted attempt may have left an unconfirmed terminald
        # request. This invocation authorizes only the components selected above.
        configurations = [(component, config) for component, config in configurations
                          if component == 'server' or terminald]
        if not configurations:
            raise ValueError('No confirmed services are ready for registration')
        helper.register(configurations, account)
        runtime = Path(account.pw_dir) / '.local/lib/aow'
        for component, _ in configurations:
            result = run_as_user(account, ['/usr/bin/env', 'node',
                str(runtime / 'latest/scripts/start-launchdaemon.mjs'), component, str(runtime)], environment)
            if result:
                raise RuntimeError(f'{component} startup verification failed (exit {result}); rerun just install --user {shlex.quote(account.pw_name)} from this administrator checkout')
    print(f'Installation completed for {account.pw_name}. Future updates can run as that account with just install or aow update.', flush=True)
    return 0


def administrator_hint(account, another_account=False):
    reason = 'Installation for another account' if another_account else 'First-time LaunchDaemon setup'
    print(f'{reason} ({account.pw_name}) requires an administrator account.', file=sys.stderr)
    print('Switch to an administrator account on this Mac and open a local terminal.', file=sys.stderr)
    print('From an administrator-owned, trusted AoW checkout with a local package, run:', file=sys.stderr)
    print(f'  just install --user {shlex.quote(account.pw_name)}', file=sys.stderr)
    print('Keep the target account unchanged. After registration, it can install updates without sudo.', file=sys.stderr)
    print('Exit code 78 means administrator setup is required; installation has not started.', file=sys.stderr)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--user', help='Target account (default: the current account)')
    parser.add_argument('--package', required=True)
    parser.add_argument('--elevated', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if sys.platform != 'darwin':
        parser.error('This installer requires macOS')
    if sys.version_info < (3, 9):
        parser.error('macOS account installation requires Python 3.9 or newer')
    if args.elevated:
        if os.geteuid() != 0 or not os.environ.get('SUDO_UID'):
            parser.error('The administrator coordinator must be started by just install')
        administrator = pwd.getpwuid(int(os.environ['SUDO_UID']))
        if administrator.pw_uid == 0 or not is_admin(administrator):
            parser.error('Installation must be initiated by a macOS administrator account')
        account = pwd.getpwnam(args.user)
        if account.pw_uid == 0:
            parser.error('AoW must run as a non-root account')
        return install_as_administrator(account, args.package, administrator.pw_uid)
    caller = pwd.getpwuid(os.getuid())
    if caller.pw_uid == 0:
        parser.error('Run just install as your normal macOS account, without sudo')
    account = pwd.getpwnam(args.user) if args.user else caller
    if account.pw_uid == 0:
        parser.error('AoW must run as a non-root account')
    print(f'Installation account: {account.pw_name} (HOME={account.pw_dir})', flush=True)
    if account.pw_uid == caller.pw_uid:
        checked = subprocess.run(['node', str(SCRIPT_DIR / 'launchd-mode.mjs'), 'check-install',
                                  str(Path(account.pw_dir) / '.local/lib/aow')],
                                 capture_output=True, text=True)
        if checked.returncode:
            print(checked.stderr, end='', file=sys.stderr)
            return checked.returncode
        status = json.loads(checked.stdout)
        if not status['registrationRequired']:
            return subprocess.run(['/bin/bash', str(SCRIPT_DIR / 'install-release.sh'),
                                   '--package', args.package]).returncode
    if not is_admin(caller):
        administrator_hint(account, account.pw_uid != caller.pw_uid)
        return 78
    require_local_terminal()
    script = trusted_path(__file__, caller.pw_uid)
    package = trusted_path(args.package, caller.pw_uid)
    return subprocess.run(['/usr/bin/sudo', '-k', '/usr/bin/python3', '-I', str(script),
                           '--elevated', '--user', account.pw_name, '--package', str(package)]).returncode


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, RuntimeError, EOFError, subprocess.SubprocessError) as error:
        print(f'error: {error}', file=sys.stderr)
        sys.exit(1)
