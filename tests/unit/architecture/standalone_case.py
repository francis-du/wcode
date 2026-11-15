"""Shared temporary-workspace fixture for the OSS source-package safety suites."""
import tempfile
import unittest
from pathlib import Path


class StandaloneCase(unittest.TestCase):
    """A private temporary directory that is removed even when a case fails."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="wcode-package-test-")
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
