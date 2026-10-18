"""Exercise eviction boundaries with disposable build artifacts."""

import fcntl
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "cache", Path(__file__).with_name("prune-build-cache.py")
)
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)


class CacheTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="pebble-cache-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "target" / "debug"
        self.root.mkdir(parents=True)

    def file(self, name, timestamp):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"x" * 8192)
        os.utime(path, (timestamp, timestamp))
        return path

    def artifact(self, hash_value, timestamp):
        fp = self.file(f".fingerprint/example-{hash_value}/lib-example", timestamp)
        lib = self.file(f"deps/libexample-{hash_value}.rlib", timestamp)
        return fp, lib

    def size(self):
        return sum({(s.st_dev, s.st_ino): s.st_blocks * 512
                    for s in cache.inventory(self.root).values()}.values())

    def test_evicts_old_group_and_stops_at_budget(self):
        old = self.artifact("a" * 16, 100)
        new = self.artifact("b" * 16, 200)
        limit = self.size() - sum(p.stat().st_blocks * 512 for p in old)
        cache.prune(self.root, limit)
        self.assertTrue(all(not p.exists() for p in old))
        self.assertTrue(all(p.exists() for p in new))
        self.assertLessEqual(self.size(), limit)

    def test_recent_access_preserves_old_build(self):
        used = self.artifact("a" * 16, 100)
        older_use = self.artifact("b" * 16, 200)
        os.utime(used[0], (300, 100))
        cache.prune(self.root, self.size() // 2)
        self.assertTrue(all(p.exists() for p in used))
        self.assertTrue(all(not p.exists() for p in older_use))

    def test_incremental_eviction_preserves_outputs_and_hardlinks(self):
        old = self.artifact("a" * 16, 100)
        final = self.root / "libexample.rlib"
        os.link(old[1], final)
        inc = self.file("incremental/example-session/cache.bin", 200)
        unknown = self.file("keep.txt", 100)
        cache.prune(self.root, final.stat().st_blocks * 512 + unknown.stat().st_blocks * 512)
        self.assertTrue(final.exists())
        self.assertTrue(unknown.exists())
        self.assertFalse(inc.exists())
        self.assertTrue(all(not p.exists() for p in old))

    def test_dry_run_and_under_budget_do_not_delete(self):
        paths = self.artifact("a" * 16, 100)
        cache.prune(self.root, 0, dry_run=True)
        cache.prune(self.root, self.size())
        self.assertTrue(all(p.exists() for p in paths))

    def test_each_cargo_lock_prevents_cleanup(self):
        paths = self.artifact("a" * 16, 100)
        for name in cache.LOCKS:
            with (self.root / name).open("w") as handle:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
                cache.prune(self.root, 0)
                self.assertTrue(all(p.exists() for p in paths))

    def test_symlinks_do_not_escape_checkout(self):
        outside = Path(self.temp.name) / "outside"
        outside.mkdir()
        sentinel = outside / "keep"
        sentinel.write_text("keep")
        (self.root / "incremental").symlink_to(outside, target_is_directory=True)
        cache.prune(self.root, 0)
        self.assertTrue(sentinel.exists())
        redirected = Path(self.temp.name) / "redirected"
        redirected.symlink_to(self.root.parent, target_is_directory=True)
        with self.assertRaises(ValueError):
            cache.prune(redirected / "debug", 0)


if __name__ == "__main__":
    unittest.main()
