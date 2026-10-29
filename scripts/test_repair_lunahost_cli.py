"""Guard the one-build CLI repair and its backup/atomic replacement behavior."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import repair_lunahost_cli as repair


class RepairLunaHostTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name) / "LunaHostCLI32.exe"
        self.original = b"MZ" + bytes(repair.OFFSET - 2) + repair.ORIGINAL_CALL + bytes(32)
        self.patched = self.original[:repair.OFFSET] + repair.REPLACEMENT + self.original[repair.OFFSET + 5:]
        self.path.write_bytes(self.original)
        self.path.chmod(0o751)
        self.backup = self.path.with_name(self.path.name + ".yomika-original")
        guard = patch.multiple(repair, ORIGINAL_SHA256=repair.digest(self.original), PATCHED_SHA256=repair.digest(self.patched))
        guard.start()
        self.addCleanup(guard.stop)

    def test_changes_only_debug_call_preserves_backup_mode_and_is_idempotent(self):
        self.assertTrue(repair.repair(self.path))
        self.assertEqual(self.path.read_bytes(), self.patched)
        self.assertEqual(len(self.patched), len(self.original))
        self.assertEqual(self.backup.read_bytes(), self.original)
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o751)
        self.assertFalse(repair.repair(self.path))
        self.assertEqual(self.backup.read_bytes(), self.original)

    def test_unknown_version_is_untouched(self):
        self.path.write_bytes(b"another CLI")
        with self.assertRaisesRegex(ValueError, "Unsupported CLI"):
            repair.repair(self.path)
        self.assertEqual(self.path.read_bytes(), b"another CLI")
        self.assertFalse(self.backup.exists())

    def test_existing_different_backup_is_never_overwritten(self):
        self.backup.write_bytes(b"different backup")
        with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
            repair.repair(self.path)
        self.assertEqual(self.path.read_bytes(), self.original)
        self.assertEqual(self.backup.read_bytes(), b"different backup")

    def test_instruction_guard_fails_without_writing(self):
        with patch.object(repair, "ORIGINAL_CALL", b"bad!!"):
            with self.assertRaisesRegex(ValueError, "Unexpected instruction"):
                repair.repair(self.path)
        self.assertEqual(self.path.read_bytes(), self.original)
        self.assertFalse(self.backup.exists())

    def test_failed_replace_keeps_original_backup_and_removes_temporary_file(self):
        with patch.object(os, "replace", side_effect=OSError("replacement failed")):
            with self.assertRaisesRegex(OSError, "replacement failed"):
                repair.repair(self.path)
        self.assertEqual(self.path.read_bytes(), self.original)
        self.assertEqual(self.backup.read_bytes(), self.original)
        self.assertEqual(set(self.path.parent.iterdir()), {self.path, self.backup})


if __name__ == "__main__":
    unittest.main()
