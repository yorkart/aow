#!/usr/bin/env python3
"""One-time, copy-only migration to ~/.aow. Run this separately before upgrading."""

import argparse
import filecmp
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import stat
import sys
import tempfile


def environment_path(value):
    """Use quoting understood literally by both systemd and AoW's env readers."""
    if any(character in value for character in '\r\n\0'):
        raise ValueError('Application home cannot contain a newline or NUL in server.env')
    if '"' not in value and '\\' not in value:
        return f'"{value}"'
    if "'" not in value:
        return f"'{value}'"
    raise ValueError('Application home cannot be represented literally in server.env')


def path_fields(relative):
    """Only AoW-owned documents and explicitly identified path fields are rewritten."""
    documents = {
        'aow-projects.json': (
            'items.*.registered_path', 'items.*.common_git_dir', 'items.*.notes_path',
            'items.*.notes_identity', 'items.*.worktree_colors.$keys', 'items.*.worktree_icons.$keys',
        ),
        'aow-settings.json': ('notes_base', 'execution_path.*', 'pinned_worktrees.*', 'pinned_directories.*'),
        'aow-agents.json': ('items.*.command', 'items.*.args.*', 'items.*.env.*'),
        'aow-acp.json': ('agent_servers.*.command', 'agent_servers.*.args.*', 'agent_servers.*.env.*'),
        'state/terminals.json': (
            'tabs.*.workspace_root', 'tabs.*.panes.*.cwd', 'tabs.*.panes.*.shell',
            'tabs.*.panes.*.arguments.*', 'tabs.*.panes.*.hosting.workspace_root',
            'tabs.*.panes.*.hosting.process.cwd', 'tabs.*.panes.*.hosting.process.pi_binding',
        ),
        'state/instances.json': ('items.*.cwd',),
        'state/inbox/executions.json': ('*.cwd',),
        'data/session-shares.json': ('shares.*.cwd', 'shares.*.transcript_path', 'shares.*.trusted_root'),
    }
    name = relative.as_posix()
    if name in documents:
        return documents[name]
    task = ('repository_path', 'workspace_path', 'launch.executable', 'launch.args.*', 'launch.environment.*')
    if re.fullmatch(r'automations/tasks/[^/]+\.json', name):
        return task
    if re.fullmatch(r'state/automations/pending/[^/]+/[^/]+\.json', name):
        return tuple('task.' + field for field in task)
    if re.fullmatch(r'state/automations/runs/[^/]+/[^/]+/session-roots\.json', name):
        return ('claude', 'codex', 'traecli', 'hermes', 'pi')
    if re.fullmatch(r'cache/acp/installations/[^/]+\.json', name):
        return ('files.*',)
    if re.fullmatch(r'data/agents/pi/[^/]+\.json', name):
        return ('cwd', 'session_file')
    return ()


def selection_directory(state):
    """Read the two top-level string fields emitted by the old config selector."""
    selector = state / 'config.toml'
    if not selector.exists():
        return state, None
    fields = {}
    for line in selector.read_text().splitlines():
        if line.lstrip().startswith('['):
            break
        match = re.fullmatch(
            r'''\s*(config-repo|config-id)\s*=\s*("(?:[^"\\]|\\.)*"|'[^']*')\s*(?:#.*)?''',
            line,
        )
        if match:
            key, value = match.groups()
            if key in fields:
                raise ValueError(f'Duplicate {key} in {selector}')
            fields[key] = value[1:-1] if value.startswith("'") else json.loads(value)
    repository, identifier = fields.get('config-repo'), fields.get('config-id')
    if not repository or not Path(repository).is_absolute() or not identifier:
        raise ValueError(f'Cannot read configuration selection from {selector}; use --config-dir')
    if identifier in ('.', '..') or '/' in identifier or '\\' in identifier:
        raise ValueError(f'Invalid config-id in {selector}')
    return Path(repository) / identifier, identifier


def environment_values(path):
    values = {}
    if path.exists():
        for line in path.read_text().splitlines():
            match = re.fullmatch(r'\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*', line)
            if match:
                key, value = match.groups()
                if len(value) >= 2 and value[0] == value[-1] and value[0] in ('"', "'"):
                    value = value[1:-1]
                values[key] = value
    return values


class Migration:
    def __init__(self, home, state, config, identifier, destination):
        self.destination = destination
        self.entries = {}
        self.directories = set()
        mapping = {
            'credentials.json': 'credentials.json',
            'notification-settings.json': 'notification-settings.json',
            'terminals.json': 'state/terminals.json',
            'instances.json': 'state/instances.json',
            'session-shares.json': 'data/session-shares.json',
            'operation-logs': 'state/operation-logs',
            'automations': 'state/automations',
            'agents': 'data/agents',
            'repos': 'data/repos',
            'data': 'data',
            'skills': 'skills',
            'im': 'state/im',
            'clipboard-images': 'cache/clipboard-images',
            'zed': 'data/zed',
        }
        self.prefixes = sorted([
            (str(config / 'acp/settings.json'), str(destination / 'aow-acp.json')),
            (str(config / 'inbox'), str(destination / 'data/inbox')),
            (str(config), str(destination)),
            (str(state / 'acp/agents'), str(destination / 'cache/acp/agents')),
            (str(state / 'acp/installations'), str(destination / 'cache/acp/installations')),
            (str(state / 'acp/registry.json'), str(destination / 'cache/acp/registry.json')),
            (str(state / 'acp'), str(destination / 'data/acp')),
            (str(state / 'automations/tasks'), str(destination / 'automations/tasks')),
            (str(state / 'cli'), str(destination / 'runtime/cli')),
            (str(state), str(destination)),
            (str(home / 'aow'), str(destination / 'data/notes')),
            *((str(state / source), str(destination / target)) for source, target in mapping.items()),
        ], key=lambda item: len(item[0]), reverse=True)
        self.collect_config(config, state)
        for source, target in mapping.items():
            self.collect(state / source, Path(target), skip={'tasks'} if source == 'automations' else set())
        acp = state / 'acp'
        if acp.is_dir():
            for child in acp.iterdir():
                if child.name == 'settings.json':
                    continue
                parent = 'cache/acp' if child.name in ('agents', 'installations', 'registry.json') else 'data/acp'
                self.collect(child, Path(parent) / child.name)
        inbox = state / 'inbox'
        if identifier:
            inbox /= identifier
        self.collect(inbox / 'executions.json', Path('state/inbox/executions.json'))
        self.collect(home / 'aow', Path('data/notes'))
        self.collect(home / '.config/aow/server.env', Path('server.env'))
        self.database = self.prepare_database()
        self.schedules = list((home / 'Library/LaunchAgents').glob('org.aow.automation.*.plist'))
        self.schedules += list((home / '.config/systemd/user').glob('aow-automation-*.service'))

    def collect_config(self, config, state):
        if not config.is_dir():
            raise ValueError(f'Configuration directory does not exist: {config}')
        for child in config.iterdir():
            name = child.name
            if name == '.git' or name.startswith('.config-') or name in ('config.toml', '__current__', 'pin.md5'):
                continue
            if config == state and name not in (
                'aow-projects.json', 'aow-agents.json', 'aow-settings.json',
                'git-platform-providers.json', 'git-platform-providers', 'acp', 'automations',
            ):
                continue
            if name == 'acp':
                self.collect(child / 'settings.json', Path('aow-acp.json'))
            elif name == 'inbox':
                self.collect(child, Path('data/inbox'))
            elif name == 'automations':
                self.collect(child / 'tasks', Path('automations/tasks'))
            else:
                self.collect(child, Path(name))

    def collect(self, source, relative, skip=frozenset()):
        if source.is_symlink():
            self.add(source, relative)
        elif source.is_dir():
            self.directories.add(relative)
            for child in sorted(source.iterdir()):
                if child.name not in skip:
                    self.collect(child, relative / child.name)
        elif source.is_file():
            self.add(source, relative)
        elif source.exists():
            raise ValueError(f'Unsupported source file: {source}')

    def add(self, source, relative):
        if relative in self.entries:
            raise ValueError(f'Multiple sources for {relative}')
        self.entries[relative] = source

    def relocate(self, value):
        if isinstance(value, str):
            candidates = [value]
            if value.startswith('/') and '\n' not in value and '\0' not in value:
                candidates.append(str(Path(value).resolve()))
            for candidate in candidates:
                for before, after in self.prefixes:
                    if candidate == before or candidate.startswith(before + '/'):
                        return after + candidate[len(before):]
        elif isinstance(value, list):
            return [self.relocate(item) for item in value]
        elif isinstance(value, dict):
            return {key: self.relocate(item) for key, item in value.items()}
        return value

    def relocate_json(self, original, fields):
        """Edit selected string tokens while preserving JSONC comments and all other bytes."""
        selectors = [field.split('.') for field in fields]
        tokens = iter(re.finditer(r'"(?:[^"\\]|\\.)*"|//[^\r\n]*|/\*[\s\S]*?\*/|[{}\[\],:]|[^\s{}\[\],:/]+|[^\s]', original))
        tokens = iter(token for token in tokens if not token[0].startswith(('//', '/*')))
        edits = []

        def rewrite(token, path):
            if any(len(selector) == len(path) and all(key == '*' or key == part for key, part in zip(selector, path)) for selector in selectors):
                previous = json.loads(token[0])
                relocated = self.relocate(previous)
                if relocated != previous:
                    edits.append((token.start(), token.end(), json.dumps(relocated, ensure_ascii=False)))

        def walk(token, path):
            if token[0] == '{':
                token = next(tokens)
                while token[0] != '}':
                    key = json.loads(token[0])
                    rewrite(token, (*path, '$keys'))
                    if next(tokens)[0] != ':':
                        raise ValueError('Invalid JSON object')
                    walk(next(tokens), (*path, key))
                    token = next(tokens)
                    if token[0] == ',':
                        token = next(tokens)
                    elif token[0] != '}':
                        raise ValueError('Invalid JSON object separator')
            elif token[0] == '[':
                token = next(tokens)
                index = 0
                while token[0] != ']':
                    walk(token, (*path, str(index)))
                    index += 1
                    token = next(tokens)
                    if token[0] == ',':
                        token = next(tokens)
                    elif token[0] != ']':
                        raise ValueError('Invalid JSON array separator')
            elif token[0].startswith('"'):
                rewrite(token, path)
            else:
                json.loads(token[0])

        try:
            walk(next(tokens), ())
        except StopIteration as error:
            raise ValueError('Incomplete JSON document') from error
        if next(tokens, None) is not None:
            raise ValueError('Unexpected text after JSON document')
        for start, end, value in reversed(edits):
            original = original[:start] + value + original[end:]
        return original

    def content(self, source, relative):
        if relative == Path('data/acp/threads.sqlite') and self.database is not None:
            return self.database
        if relative.parts[:2] == ('data', 'repos') and (
            source.name == '.git' or '.git' in relative.parts and source.name in ('gitdir', 'commondir')
        ):
            original = source.read_text()
            prefix = 'gitdir: ' if original.startswith('gitdir: ') else ''
            path = original[len(prefix):].rstrip('\r\n')
            relocated = self.relocate(path)
            if relocated != path:
                return (prefix + relocated + original[len(prefix) + len(path):]).encode()
            return None
        if relative == Path('server.env'):
            lines = []
            for line in source.read_text().splitlines(keepends=True):
                match = re.match(r'\s*([A-Za-z_][A-Za-z0-9_]*)\s*=', line)
                key = match[1] if match else None
                if key in ('XDG_STATE_HOME', 'XDG_RUNTIME_DIR'):
                    continue
                if key in ('AOW_STATE_DIR', 'AOW_SERVER_STATE_DIR'):
                    line = f'{key}={environment_path(str(self.destination))}\n'
                lines.append(line)
            return ''.join(lines).encode()
        fields = path_fields(relative)
        if fields:
            try:
                original = source.read_bytes().decode('utf-8')
                if relative != Path('aow-acp.json'):
                    json.loads(original)
            except (ValueError, UnicodeError):
                return None
            relocated = self.relocate_json(original, fields)
            if relocated != original:
                return relocated.encode()
        return None

    def prepare_database(self):
        """Relocate ACP workspace references in a private copy, including its WAL."""
        relative = Path('data/acp/threads.sqlite')
        source = self.entries.get(relative)
        if source is None or source.is_symlink():
            return None
        with source.open('rb') as stream:
            if stream.read(16) != b'SQLite format 3\0':
                return None
        with tempfile.TemporaryDirectory(prefix='aow-migration-') as temporary:
            database = Path(temporary) / 'threads.sqlite'
            shutil.copyfile(source, database)
            wal = Path(str(source) + '-wal')
            if wal.is_file():
                shutil.copyfile(wal, Path(str(database) + '-wal'))
            connection = sqlite3.connect(database)
            try:
                tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
                if 'sidebar_threads' in tables:
                    for thread_id, folders in connection.execute('SELECT thread_id, folder_paths FROM sidebar_threads').fetchall():
                        if folders:
                            previous = json.loads(folders)
                            relocated = self.relocate(previous)
                            if relocated != previous:
                                connection.execute('UPDATE sidebar_threads SET folder_paths=? WHERE thread_id=?', (json.dumps(relocated), thread_id))
                if 'acp_thread_snapshots' in tables:
                    for thread_id, data in connection.execute("SELECT thread_id, data FROM acp_thread_snapshots WHERE data_type='json'").fetchall():
                        previous = json.loads(data)
                        relocated = dict(previous)
                        if 'cwd' in relocated:
                            relocated['cwd'] = self.relocate(relocated['cwd'])
                        if relocated != previous:
                            connection.execute('UPDATE acp_thread_snapshots SET data=? WHERE thread_id=?', (json.dumps(relocated, ensure_ascii=False).encode(), thread_id))
                connection.commit()
                connection.execute('PRAGMA wal_checkpoint(TRUNCATE)')
                connection.execute('PRAGMA journal_mode=DELETE')
            finally:
                connection.close()
            content = database.read_bytes()
        for suffix in ('-wal', '-shm'):
            sidecar = Path(str(relative) + suffix)
            self.entries.pop(sidecar, None)
            target = self.destination / sidecar
            if target.exists() or target.is_symlink():
                raise ValueError(f'Destination conflict: {target}')
        return content

    def check_parent(self, path):
        while path != self.destination.parent:
            if path.is_symlink() or (path.exists() and not path.is_dir()):
                raise ValueError(f'Destination parent is not a regular directory: {path}')
            path = path.parent

    def link_target(self, source, destination):
        target = os.readlink(source)
        if os.path.isabs(target):
            return self.relocate(target)
        # Resolve relative to the source link before accounting for the new layout.
        absolute = os.path.abspath(source.parent / target)
        return os.path.relpath(self.relocate(absolute), destination.parent)

    def make_directory(self, directory):
        self.destination.mkdir(mode=0o700, exist_ok=True)
        parent = self.destination
        for part in directory.relative_to(self.destination).parts:
            parent /= part
            parent.mkdir(mode=0o700, exist_ok=True)

    def matches(self, source, target, content):
        if source.is_symlink():
            return target.is_symlink() and os.readlink(target) == self.link_target(source, target)
        return not target.is_symlink() and target.is_file() and (
            target.read_bytes() == content if content is not None else filecmp.cmp(source, target, shallow=False)
        )

    def run(self, dry_run):
        pending = []
        for relative in sorted(self.directories):
            self.check_parent(self.destination / relative)
        for relative, source in sorted(self.entries.items()):
            target = self.destination / relative
            self.check_parent(target.parent)
            content = None if source.is_symlink() else self.content(source, relative)
            if target.exists() or target.is_symlink():
                if not self.matches(source, target, content):
                    raise ValueError(f'Destination conflict: {target}')
            else:
                pending.append((source, target, content))
        print(f'{len(self.entries)} files checked; {len(pending)} to copy into {self.destination}')
        if self.schedules:
            print(f'{len(self.schedules)} existing OS automation registrations are outside this file copy; re-save those tasks after upgrading to register the new directory.')
        if dry_run:
            return
        for relative in sorted(self.directories):
            self.make_directory(self.destination / relative)
        for source, target, content in pending:
            self.make_directory(target.parent)
            if source.is_symlink():
                target.symlink_to(self.link_target(source, target))
            else:
                descriptor, temporary = tempfile.mkstemp(prefix='.migration-', dir=target.parent)
                try:
                    with os.fdopen(descriptor, 'wb') as output:
                        if content is None:
                            with source.open('rb') as input_file:
                                shutil.copyfileobj(input_file, output)
                        else:
                            output.write(content)
                        output.flush()
                        os.fsync(output.fileno())
                    os.chmod(temporary, 0o700 if source.stat().st_mode & stat.S_IXUSR else 0o600)
                    os.link(temporary, target)
                finally:
                    os.unlink(temporary)
            if not self.matches(source, target, content):
                raise ValueError(f'Copy verification failed: {target}')
        print('Migration complete. Original files were preserved. The installation can now be upgraded.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--home', type=Path, default=Path.home(), help='OS account home')
    parser.add_argument('--state-dir', type=Path, help='Old AoW state directory')
    parser.add_argument('--config-dir', type=Path, help='Selected old configuration directory')
    parser.add_argument('--destination', type=Path, help='New application home (default: HOME/.aow)')
    parser.add_argument('--dry-run', action='store_true', help='Check sources and conflicts without writing')
    args = parser.parse_args()
    home = args.home.expanduser().resolve()
    configured = environment_values(home / '.config/aow/server.env')
    state = (args.state_dir or Path(
        os.environ.get('AOW_SERVER_STATE_DIR') or configured.get('AOW_SERVER_STATE_DIR')
        or os.environ.get('AOW_STATE_DIR') or configured.get('AOW_STATE_DIR')
        or str(Path(os.environ.get('XDG_STATE_HOME') or configured.get('XDG_STATE_HOME') or home / '.local/state') / 'aow')
    )).expanduser().resolve()
    config, identifier = (args.config_dir, args.config_dir.name) if args.config_dir else selection_directory(state)
    config = config.expanduser().resolve()
    destination = (args.destination or home / '.aow').expanduser().absolute()
    resolved_destination = destination.resolve()
    for source in (state, config, home / 'aow'):
        if resolved_destination == source or source in resolved_destination.parents or resolved_destination in source.parents:
            raise ValueError(f'Source and destination overlap: {source}, {destination}')
    Migration(home, state, config, identifier, destination).run(args.dry_run)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, sqlite3.Error) as error:
        print(f'Migration failed: {error}', file=sys.stderr)
        sys.exit(1)
