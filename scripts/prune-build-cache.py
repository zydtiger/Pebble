"""Trim this checkout's Debug cache using filesystem times as approximate LRU."""

import argparse
from collections import Counter, defaultdict
from contextlib import ExitStack
import fcntl
import os
from pathlib import Path
import re
import shutil


GIB = 1024**3
ARTIFACT = re.compile(r"-([0-9a-f]{16})(?:\.|$)")
LOCKS = (".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock")


def inventory(root):
    """Stat only: reading payloads would change the access times we rank by."""
    files = {}
    for directory, dirs, names in os.walk(root, followlinks=False):
        dirs[:] = [name for name in dirs if not (Path(directory) / name).is_symlink()]
        for name in names:
            path = Path(directory) / name
            if not path.is_symlink():
                files[path] = path.stat()
    return files


def candidates(root, files):
    groups = defaultdict(list)
    fingerprints = root / ".fingerprint"
    known = set()
    if fingerprints.is_dir() and not fingerprints.is_symlink():
        for path in fingerprints.iterdir():
            match = ARTIFACT.search(path.name)
            if match and path.is_dir() and not path.is_symlink():
                known.add(match[1])
    for path, stat in files.items():
        parts = path.relative_to(root).parts
        if len(parts) < 2:
            continue  # Keep final top-level libraries and executables.
        if parts[0] == "incremental":
            key = ("incremental", parts[1])
        elif parts[0] in (".fingerprint", "deps", "build"):
            match = ARTIFACT.search(parts[1])
            if not match or match[1] not in known:
                continue
            key = ("artifact", match[1])
        else:
            continue
        groups[key].append((path, stat))
    # Keep related fingerprint, library, object and build-script files together.
    # atime can be coarse or disabled; mtime also protects newly built artifacts.
    return sorted(groups.values(), key=lambda group: max(
        max(stat.st_atime_ns, stat.st_mtime_ns) for _, stat in group
    ))


def trim(root, limit, dry_run=False):
    files = inventory(root)
    refs = Counter((s.st_dev, s.st_ino) for s in files.values())
    sizes = {(s.st_dev, s.st_ino): s.st_blocks * 512 for s in files.values()}
    total = sum(sizes.values())
    before = total
    removed = 0
    if total <= limit:
        return
    for group in candidates(root, files):
        if total <= limit:
            break
        roots = set()
        for path, stat in group:
            parts = path.relative_to(root).parts
            roots.add(root / parts[0] / parts[1])
        for path in sorted(roots):
            if not dry_run:
                if path.is_dir():
                    shutil.rmtree(path)
                else:
                    path.unlink()
        for _, stat in group:
            inode = (stat.st_dev, stat.st_ino)
            refs[inode] -= 1
            if refs[inode] == 0:
                total -= sizes[inode]
        removed += 1
    action = "Would trim" if dry_run else "Trimmed"
    print(f"{action} Debug cache: {before / GIB:.2f} → {total / GIB:.2f} GiB "
          f"({removed} artifact/incremental groups; approximate LRU).")
    if total > limit:
        print("Retained final outputs/unrecognized files exceed the budget; "
              "no full cargo clean was performed.")


def prune(root, limit, dry_run=False):
    # Never follow a target/debug redirect into another checkout or shared cache.
    if root.parent.is_symlink() or root.is_symlink():
        raise ValueError("Refusing to prune a symlinked target/debug directory")
    if not root.is_dir():
        return
    with ExitStack() as stack:
        try:
            for name in LOCKS:
                descriptor = os.open(root / name, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o644)
                handle = stack.enter_context(os.fdopen(descriptor, "r+"))
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("Debug cache cleanup skipped: Cargo is using this directory.")
            return
        trim(root, limit, dry_run)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--max-gib", type=int, default=20)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    if args.max_gib <= 0:
        parser.error("--max-gib must be positive")
    root = Path(__file__).resolve().parent.parent / "target" / "debug"
    prune(root, args.max_gib * GIB, args.dry_run)


if __name__ == "__main__":
    main()
