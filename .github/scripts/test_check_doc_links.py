#!/usr/bin/env python3
"""Self-tests for check_doc_links.py."""

from __future__ import annotations

import os
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_doc_links as cdl  # noqa: E402


# ---------------------------------------------------------------------------
# Slug generation
# ---------------------------------------------------------------------------

class TestGithubSlug(unittest.TestCase):

    def test_plain_heading(self):
        self.assertEqual(cdl.github_slug('Hello World'), 'hello-world')

    def test_backtick_content_kept(self):
        self.assertEqual(cdl.github_slug('Use `foo_bar` here'), 'use-foo_bar-here')

    def test_special_chars_stripped(self):
        # Parentheses, punctuation, etc. stripped
        self.assertEqual(cdl.github_slug('Tooling finding — soroban-cost-estimator v0.1.0 (beta)'),
                         'tooling-finding--soroban-cost-estimator-v010-beta')

    def test_leading_hashes_stripped(self):
        self.assertEqual(cdl.github_slug('## My Section'), 'my-section')

    def test_numbers_kept(self):
        self.assertEqual(cdl.github_slug('Section 3.1'), 'section-31')

    def test_already_lowercase(self):
        self.assertEqual(cdl.github_slug('already-lower'), 'already-lower')

    def test_multiple_spaces_collapse(self):
        self.assertEqual(cdl.github_slug('a  b   c'), 'a-b-c')

    def test_unicode_letters_kept(self):
        # Unicode word characters survive
        slug = cdl.github_slug('Résumé')
        self.assertIn('sum', slug)

    def test_empty(self):
        self.assertEqual(cdl.github_slug(''), '')


# ---------------------------------------------------------------------------
# Heading extraction
# ---------------------------------------------------------------------------

class TestExtractSlugs(unittest.TestCase):

    def test_basic(self):
        md = textwrap.dedent("""\
            # Top level
            Some text.
            ## Sub section
            ### Deep one
        """)
        slugs = cdl.extract_slugs(md)
        self.assertIn('top-level', slugs)
        self.assertIn('sub-section', slugs)
        self.assertIn('deep-one', slugs)

    def test_ignores_indented(self):
        # Indented headings are not ATX headings in strict CommonMark
        md = '    ## Not a heading\n'
        # The regex anchors at start-of-line so this shouldn't match.
        slugs = cdl.extract_slugs(md)
        self.assertNotIn('not-a-heading', slugs)

    def test_backtick_in_heading(self):
        md = '## The `foo` function\n'
        slugs = cdl.extract_slugs(md)
        self.assertIn('the-foo-function', slugs)

    def test_duplicate_headings(self):
        md = '## Foo\n## Foo\n'
        slugs = cdl.extract_slugs(md)
        self.assertIn('foo', slugs)


# ---------------------------------------------------------------------------
# Link splitting
# ---------------------------------------------------------------------------

class TestSplitTarget(unittest.TestCase):

    def test_path_and_anchor(self):
        self.assertEqual(cdl._split_target('./foo.md#bar'), ('./foo.md', 'bar'))

    def test_anchor_only(self):
        self.assertEqual(cdl._split_target('#bar'), ('', 'bar'))

    def test_path_only(self):
        self.assertEqual(cdl._split_target('./foo.md'), ('./foo.md', ''))

    def test_parent_dir(self):
        self.assertEqual(cdl._split_target('../CHANGELOG.md#section-1'),
                         ('../CHANGELOG.md', 'section-1'))

    def test_whitespace_stripped(self):
        self.assertEqual(cdl._split_target(' ./a.md#b '), ('./a.md', 'b'))


# ---------------------------------------------------------------------------
# check_file — integration tests using temp directories
# ---------------------------------------------------------------------------

class TestCheckFile(unittest.TestCase):

    def _make_repo(self, files: dict[str, str]) -> Path:
        """Create a temporary directory tree and return its path."""
        tmp = tempfile.mkdtemp()
        root = Path(tmp)
        for rel, content in files.items():
            p = root / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding='utf-8')
        return root

    # -- passing cases -------------------------------------------------------

    def test_valid_anchor_same_file(self):
        root = self._make_repo({'README.md': '## Intro\nSee [here](#intro).\n'})
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(issues, [])

    def test_valid_relative_link(self):
        root = self._make_repo({
            'README.md': 'See [other](./docs/other.md).\n',
            'docs/other.md': '# Other\n',
        })
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(issues, [])

    def test_valid_relative_link_with_anchor(self):
        root = self._make_repo({
            'README.md': 'See [gas](./docs/gas.md#results).\n',
            'docs/gas.md': '# Top\n## Results\n',
        })
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(issues, [])

    def test_absolute_url_ignored(self):
        root = self._make_repo({
            'README.md': '[gh](https://github.com/example)\n',
        })
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(issues, [])

    def test_parent_dir_link(self):
        root = self._make_repo({
            'docs/child.md': 'See [root](../README.md).\n',
            'README.md': '# Root\n',
        })
        issues = cdl.check_file(root / 'docs' / 'child.md', root, {})
        self.assertEqual(issues, [])

    # -- failing cases -------------------------------------------------------

    def test_missing_file(self):
        root = self._make_repo({'README.md': '[gone](./missing.md)\n'})
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(len(issues), 1)
        self.assertIn('missing.md', issues[0].reason)
        self.assertEqual(issues[0].line_number, 1)

    def test_missing_anchor_in_same_file(self):
        root = self._make_repo({'README.md': '# Real\nSee [bad](#nope).\n'})
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(len(issues), 1)
        self.assertIn('nope', issues[0].reason)

    def test_missing_anchor_in_other_file(self):
        root = self._make_repo({
            'README.md': 'See [bad](./docs/other.md#ghost).\n',
            'docs/other.md': '# Real heading\n',
        })
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(len(issues), 1)
        self.assertIn('ghost', issues[0].reason)

    def test_line_number_reported(self):
        root = self._make_repo({
            'README.md': 'line1\nline2\n[bad](./gone.md)\nline4\n',
        })
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(issues[0].line_number, 3)

    # -- edge cases ----------------------------------------------------------

    def test_anchor_only_link_valid(self):
        root = self._make_repo({'README.md': '## Setup\nSee [this](#setup).\n'})
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(issues, [])

    def test_multiple_issues_per_file(self):
        root = self._make_repo({
            'README.md': '[a](./a.md)\n[b](./b.md)\n',
        })
        issues = cdl.check_file(root / 'README.md', root, {})
        self.assertEqual(len(issues), 2)


# ---------------------------------------------------------------------------
# Full run
# ---------------------------------------------------------------------------

class TestRun(unittest.TestCase):

    def _make_repo(self, files: dict[str, str]) -> Path:
        tmp = tempfile.mkdtemp()
        root = Path(tmp)
        for rel, content in files.items():
            p = root / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding='utf-8')
        return root

    def test_clean_repo_exits_zero(self):
        root = self._make_repo({
            'README.md': '# Readme\nSee [guide](./docs/guide.md).\n',
            'docs/guide.md': '## Setup\nAll good.\n',
        })
        self.assertEqual(cdl.run(root), 0)

    def test_broken_link_exits_one(self):
        root = self._make_repo({'README.md': '[gone](./missing.md)\n'})
        self.assertEqual(cdl.run(root), 1)

    def test_bad_root_exits_three(self):
        self.assertEqual(cdl.run(Path('/nonexistent/path/xyz')), 3)

    def test_skip_dirs_honoured(self):
        # Links inside target/ should not be scanned.
        root = self._make_repo({
            'README.md': '# Root\n',
            'target/debug/some.md': '[gone](./nowhere.md)\n',
        })
        self.assertEqual(cdl.run(root), 0)


if __name__ == '__main__':
    unittest.main()
