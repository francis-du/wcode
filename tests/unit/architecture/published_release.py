"""Behavioral regressions for the production archive verifier, without remote requests."""
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
with patch.dict(os.environ, {
    'GITHUB_REPOSITORY': 'owner/repository', 'RELEASE_TAG': 'v0.9.0',
    'CANDIDATE_SHA': 'a' * 40, 'CANDIDATE_VERSION': '0.9.0',
    'GITHUB_RUN_ID': '10', 'GITHUB_RUN_ATTEMPT': '1',
}):
    spec = importlib.util.spec_from_file_location('published_release', ROOT / 'tests/published_release.py')
    verifier = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(verifier)


def archive(kind, entries):
    output = io.BytesIO()
    if kind == 'tar':
        with tarfile.open(fileobj=output, mode='w:gz') as bundle:
            for name, payload, symbolic in entries:
                member = tarfile.TarInfo(name)
                if symbolic:
                    member.type = tarfile.SYMTYPE
                    member.linkname = 'wcode'
                    bundle.addfile(member)
                else:
                    member.size = len(payload)
                    bundle.addfile(member, io.BytesIO(payload))
    else:
        with zipfile.ZipFile(output, 'w') as bundle:
            for name, payload, symbolic in entries:
                member = zipfile.ZipInfo(name)
                if symbolic:
                    member.external_attr = 0o120777 << 16
                bundle.writestr(member, payload)
    return output.getvalue()


class PublishedArchiveTests(unittest.TestCase):
    def test_runtime_arguments_are_accepted_by_the_actual_binary(self):
        binary = os.environ['WCODE_RELEASE_TEST_BINARY']
        with tempfile.TemporaryDirectory(prefix='wcode-release-cli-') as workspace:
            args = verifier.runtime_arguments(binary, workspace, 19789) + ['--show-config']
            result = subprocess.run(args, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            config = json.loads(result.stdout)
            self.assertTrue(config['preview'])
            self.assertFalse(config['runtime_started'])
            self.assertFalse(config['monitor_requested'])
            self.assertFalse(config['menu_bar_requested'])
            for key in ('write_enabled', 'exec_enabled', 'semantic_enabled', 'full_access'):
                self.assertFalse(config['permissions'][key])
            self.assertNotIn('--open', args)

    def test_unknown_arguments_fail_without_starting_runtime(self):
        binary = os.environ['WCODE_RELEASE_TEST_BINARY']
        result = subprocess.run([binary, '--wcode-release-invalid-option'], capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 2)
        self.assertIn(b'unexpected argument', result.stderr)

    def test_checksum_manifest_requires_every_package_exactly_once(self):
        lines = [f'{"a" * 64}  {name}' for name in sorted(verifier.EXPECTED)]
        valid = '\n'.join(lines).encode()
        self.assertEqual(set(verifier.parse_checksums(valid)), verifier.EXPECTED)
        for invalid in [b'', '\n'.join(lines[:-1]).encode(), valid + b'\n' + lines[0].encode(),
                        valid + b'\n' + b'b' * 64 + b'  wcode-foreign.tar.gz', valid.replace(b'  ', b' '),
                        valid.replace(b'a', b'G', 1)]:
            with self.subTest(invalid=invalid[:70]):
                with self.assertRaises((RuntimeError, UnicodeError)):
                    verifier.parse_checksums(invalid)

    def test_valid_archives_return_the_declared_binary(self):
        for kind, name, binary in [('tar', 'wcode-linux-x86_64.tar.gz', 'wcode'),
                                   ('zip', 'wcode-windows-x86_64.zip', 'wcode.exe')]:
            entries = [(binary, b'actual-payload', False), ('README.md', b'docs', False), ('LICENSE', b'license', False)]
            self.assertEqual(verifier.binary_bytes(archive(kind, entries), name), b'actual-payload')

    def test_archives_reject_traversal_links_directories_and_duplicates(self):
        for kind, name, binary in [('tar', 'wcode-linux-x86_64.tar.gz', 'wcode'),
                                   ('zip', 'wcode-windows-x86_64.zip', 'wcode.exe')]:
            entries = [(binary, b'payload', False), ('README.md', b'docs', False), ('LICENSE', b'license', False)]
            for extra in [('../escaped', b'x', False), ('subdir/foreign', b'x', False),
                          ('foreign/', b'', False), ('link', b'x', True), (binary, b'duplicate', False)]:
                with self.subTest(kind=kind, member=extra[0]):
                    with self.assertRaises(RuntimeError):
                        verifier.binary_bytes(archive(kind, entries + [extra]), name)

    def test_incomplete_archives_are_rejected(self):
        for kind, name, binary in [('tar', 'wcode-linux-x86_64.tar.gz', 'wcode'),
                                   ('zip', 'wcode-windows-x86_64.zip', 'wcode.exe')]:
            with self.assertRaises(RuntimeError):
                verifier.binary_bytes(archive(kind, [(binary, b'payload', False)]), name)

    def test_publication_is_bound_to_the_original_workflow_and_tagged_candidate(self):
        run = {'id': verifier.RELEASE_RUN, 'repository': {'full_name': verifier.REPO},
               'run_attempt': verifier.RUN_ATTEMPT, 'path': '.github/workflows/release.yml',
               'event': 'push', 'head_sha': verifier.TARGET, 'head_branch': verifier.TAG}
        self.assertEqual(verifier.validate_publication(run), 'tag_push')
        for change in [{'id': 99}, {'repository': {'full_name': 'foreign/repo'}}, {'run_attempt': 99},
                       {'path': '.github/workflows/other.yml'}, {'event': 'pull_request'},
                       {'head_sha': 'b' * 40}, {'head_branch': 'v0.8.5'},
                       {'event': 'workflow_dispatch', 'head_branch': 'main', 'head_sha': 'b' * 40}]:
            with self.subTest(change=change):
                with self.assertRaises(RuntimeError):
                    verifier.validate_publication({**run, **change})
        dispatched = {**run, 'event': 'workflow_dispatch', 'head_branch': 'main', 'head_sha': 'b' * 40}
        self.assertEqual(verifier.validate_publication(dispatched, allow_current_dispatch=True),
                         'current_workflow_validated_candidate')

    def test_tag_identity_follows_a_bounded_annotated_tag_chain(self):
        responses = [{'object': {'type': 'tag', 'sha': 'b' * 40}},
                     {'object': {'type': 'commit', 'sha': 'a' * 40}}]
        with patch.object(verifier, 'get_api', side_effect=responses):
            self.assertEqual(verifier.tag_commit(), 'a' * 40)

    def test_tag_identity_rejects_noncommits_invalid_shas_and_excessive_depth(self):
        for responses in [[{'object': {'type': 'tree', 'sha': 'b' * 40}}],
                          [{'object': {'type': 'commit', 'sha': '../wrong'}}],
                          [{'object': {'type': 'tag', 'sha': 'b' * 40}}] * 5]:
            with patch.object(verifier, 'get_api', side_effect=responses):
                with self.assertRaises(RuntimeError):
                    verifier.tag_commit()


if __name__ == '__main__':
    unittest.main(verbosity=2)
