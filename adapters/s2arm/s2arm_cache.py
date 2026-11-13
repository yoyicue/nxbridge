"""Private, single-writer shader caches for prepared S2Arm builds.

No subprocesses: the launcher must retain its inherited Mach media endpoint.
The work directory is caller-owned; hostile concurrent changes by the same UID
are outside this boundary. Existing symlinks are rejected before native writers
receive any path. Cache data is disposable, never a source of player saves.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import tempfile


def digest(path):
    before = path.stat()
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''): h.update(block)
    after = path.stat()
    if (before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_ino, after.st_size, after.st_mtime_ns):
        raise ValueError(f'build changed while hashing: {path.name}')
    return h.hexdigest()


def identity(engine, game, manifest):
    # Do not scan all RomFS assets on each launch. Module/metadata hashes scope
    # the game build; shader content keys + upstream chain/translator checks
    # still validate each translated shader. Metal archives additionally check
    # the actual device name/OS/chain in par_identity before opening any file.
    files = set(game.glob('*.patched.bin')) | {game / 'main.npdm'}
    for name in ('romfs.s2aidx', '.stage_rewrite.deps'):
        if (game / name).is_file(): files.add(game / name)
    direct = game / 'shaders/direct'
    if direct.is_dir():
        files.update(direct.rglob('lib.idx'))
        files.update(direct.rglob('inputs.tsv'))
        files.update(p for p in direct.glob('*.tsv') if p.name != 'pso_manifest.tsv')
    uname = os.uname()
    binaries = {engine / 'build/s2arm-host'} | set((engine / 'build').glob('*.dylib'))
    return {
        'schema_version': 1,
        'prepared_source': json.loads(json.dumps(manifest)),
        'binaries': {str(p.relative_to(engine)): digest(p) for p in sorted(binaries)},
        'game': {str(p.relative_to(game)): digest(p) for p in sorted(files)},
        'platform': {'system': uname.sysname, 'release': uname.release,
                     'version': uname.version, 'machine': uname.machine},
    }


def key(identity):
    return hashlib.sha256(json.dumps(identity, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def game_key(identity):
    """Digest of the tracked game inputs, independent of host binaries."""
    return hashlib.sha256(json.dumps(identity['game'], sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def location(work, identity):
    return work / '.s2arm-cache' / key(identity)


def check_directory(path):
    if path.is_symlink(): raise ValueError(f'cache symlink refused: {path.name}')
    if not path.is_dir(): raise ValueError(f'cache path is not a directory: {path.name}')


def check_tree(path):
    check_directory(path)
    for root, dirs, files in os.walk(path, followlinks=False):
        for name in dirs + files:
            p = Path(root) / name
            mode = p.lstat().st_mode
            if stat.S_ISLNK(mode): raise ValueError(f'cache symlink refused: {p.name}')
            if not (stat.S_ISDIR(mode) or stat.S_ISREG(mode)):
                raise ValueError(f'cache special file refused: {p.name}')


def recover_manifest(path):
    # A crash can leave the final append incomplete. Only newline-terminated
    # records are published back; native warm_parse validates their contents.
    if not path.exists(): return
    data = path.read_bytes()
    if data and not data.endswith(b'\n'):
        fd, name = tempfile.mkstemp(prefix='.pso-repair-', dir=path.parent)
        temp = Path(name)
        try:
            with os.fdopen(fd, 'wb') as stream:
                stream.write(data[:data.rfind(b'\n') + 1]); stream.flush(); os.fsync(stream.fileno())
            os.replace(temp, path)
        finally:
            temp.unlink(missing_ok=True)


def acquire(work, identity):
    entry = location(work, identity)
    root = entry.parent
    root.mkdir(mode=0o700, exist_ok=True)
    check_directory(root)
    fd = os.open(root / (entry.name + '.lock'), os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode): raise ValueError('cache lock must be a regular file')
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if not entry.exists() and not entry.is_symlink():
            stage = Path(tempfile.mkdtemp(prefix='.staging-', dir=root))
            try:
                for name in ('shaders', 'gal'): (stage / name).mkdir(mode=0o700)
                with (stage / 'identity.json').open('x') as stream:
                    json.dump(identity, stream, sort_keys=True, indent=2)
                    stream.write('\n'); stream.flush(); os.fsync(stream.fileno())
                # The same entry lock serializes publication; an incomplete
                # staging directory is never considered a usable cache.
                os.rename(stage, entry)
            finally:
                if stage.exists(): shutil.rmtree(stage)
        check_tree(entry)
        if json.loads((entry / 'identity.json').read_text()) != identity:
            raise ValueError('cache identity mismatch; retain the entry for inspection')
        for name in ('shaders', 'gal'): check_directory(entry / name)
        recover_manifest(entry / 'pso_user.tsv')
        os.set_inheritable(fd, True)
        return entry, fd
    except BaseException:
        os.close(fd)
        raise
