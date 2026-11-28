#!/usr/bin/env python3
"""Markdown relative-link and anchor checker.

Walks every *.md file in the repository, resolves relative file links and
#anchor links against the filesystem and GitHub's heading-slug rules, and
reports broken ones with file:line.

Exit codes:
  0  all links resolve
  1  one or more broken links found
  3  error — the check could not complete (e.g. bad repo root argument)

Usage:
  check_doc_links.py [REPO_ROOT]

REPO_ROOT defaults to the directory two levels above this script
(i.e. the repository root when the script lives at .github/scripts/).

Standard library only — nothing to install.
"""

from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

# ---------------------------------------------------------------------------
# Heading slug generation
# ---------------------------------------------------------------------------

# GitHub's heading-slug algorithm (as of 2024):
#   1. lower-case the heading text
#   2. strip everything that isn't a letter, digit, hyphen, or space
#      (backtick spans: strip the backticks, keep the content)
#   3. replace runs of spaces with a single hyphen
#
# References:
#   https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#section-links
#   https://github.com/thlorenz/anchor-markdown-header
_STRIP_BACKTICKS = re.compile(r'`([^`]*)`')
_COLLAPSE_SPACE = re.compile(r'\s+')
_STRIP_NON_SLUG = re.compile(r'[^\w-]', re.UNICODE)


def github_slug(heading_text: str) -> str:
    """Convert a Markdown heading string to a GitHub anchor slug.

    Matches GitHub's actual algorithm (as implemented by github-slugger):
    1. strip leading ATX '#' characters and whitespace
    2. strip Markdown backtick spans (keep the inner text)
    3. lower-case
    4. replace runs of whitespace with a single hyphen
    5. remove everything that is not a word character (\\w) or a hyphen

    The order of steps 4 and 5 matters: an em-dash surrounded by spaces
    (e.g. "foo — bar") first becomes "foo-—-bar" (step 4), then the em-dash
    is stripped (step 5), yielding "foo--bar" — the double hyphen GitHub
    actually generates for that heading.
    """
    text = heading_text.strip()
    text = text.lstrip('#').strip()
    text = _STRIP_BACKTICKS.sub(r'\1', text)
    text = text.lower()
    text = _COLLAPSE_SPACE.sub('-', text.strip())
    text = _STRIP_NON_SLUG.sub('', text)
    return text


# ---------------------------------------------------------------------------
# Heading extraction
# ---------------------------------------------------------------------------

# ATX headings only (## Foo) — setext headings (underline style) are uncommon
# in this repo.
_ATX_HEADING = re.compile(r'^#{1,6}\s+(.*)')


def extract_slugs(md_text: str) -> set[str]:
    """Return the set of anchor slugs for all ATX headings in a Markdown file.

    Duplicate headings get numeric suffixes (GitHub appends -1, -2, … for
    repeated slugs) but those are rare in this repo and the checker only
    verifies that *a* heading with that slug exists, not which occurrence.
    """
    slugs: set[str] = set()
    for line in md_text.splitlines():
        m = _ATX_HEADING.match(line)
        if m:
            slugs.add(github_slug(m.group(1)))
    return slugs


# ---------------------------------------------------------------------------
# Link extraction
# ---------------------------------------------------------------------------

# Matches [text](target) where target is not an absolute URL.
# Excludes http://, https://, mailto:, ftp://, etc.
_MD_LINK = re.compile(
    r'\[(?:[^\[\]]*)\]\((?!https?://|mailto:|ftp://)([^)]+)\)',
    re.IGNORECASE,
)
_ABS_PATH = re.compile(r'^(?:[A-Za-z]:[\\/]|/)')


def _split_target(target: str) -> tuple[str, str]:
    """Split a Markdown link target into (path_part, anchor_part).

    For ``./foo.md#bar`` returns ``('./foo.md', 'bar')``.
    For ``#bar`` returns ``('', 'bar')``.
    For ``./foo.md`` returns ``('./foo.md', '')``.
    """
    target = target.strip()
    if '#' in target:
        path_part, _, anchor = target.partition('#')
        return path_part.strip(), anchor.strip()
    return target, ''


@dataclass
class LinkIssue:
    """One broken link found in a Markdown file."""

    source_file: str
    line_number: int
    raw_target: str
    reason: str


# ---------------------------------------------------------------------------
# Per-file checking
# ---------------------------------------------------------------------------

def check_file(
    md_path: Path,
    repo_root: Path,
    slug_cache: dict[Path, set[str]],
) -> list[LinkIssue]:
    """Return a list of broken links found in *md_path*."""
    issues: list[LinkIssue] = []
    try:
        text = md_path.read_text(encoding='utf-8', errors='replace')
    except OSError as exc:
        return [LinkIssue(str(md_path), 0, '', f'cannot read file: {exc}')]

    # Cache this file's own slugs for anchor self-links.
    if md_path not in slug_cache:
        slug_cache[md_path] = extract_slugs(text)

    for lineno, line in enumerate(text.splitlines(), start=1):
        for m in _MD_LINK.finditer(line):
            raw = m.group(1)
            path_part, anchor = _split_target(raw)

            # Skip absolute paths (not relative).
            if _ABS_PATH.match(path_part):
                continue

            # Resolve the target file.
            if path_part:
                target_path = (md_path.parent / path_part).resolve()
            else:
                target_path = md_path  # anchor-only link → same file

            # --- file existence check ---
            if path_part and not target_path.exists():
                issues.append(LinkIssue(
                    str(md_path.relative_to(repo_root)),
                    lineno,
                    raw,
                    f'target file not found: {path_part}',
                ))
                continue  # can't check anchor without the file

            # --- anchor check ---
            if anchor:
                if target_path not in slug_cache:
                    if target_path.exists():
                        try:
                            slug_cache[target_path] = extract_slugs(
                                target_path.read_text(
                                    encoding='utf-8', errors='replace'
                                )
                            )
                        except OSError:
                            slug_cache[target_path] = set()
                    else:
                        slug_cache[target_path] = set()

                if anchor not in slug_cache.get(target_path, set()):
                    # Try numeric-suffix variant (GitHub deduplication)
                    # — if any slug starting with the base exists, accept it.
                    base_slug = anchor.rstrip('-0123456789')
                    if base_slug and any(
                        s == anchor or s.startswith(base_slug + '-')
                        for s in slug_cache.get(target_path, set())
                    ):
                        continue
                    rel_target = (
                        str(target_path.relative_to(repo_root))
                        if target_path.is_relative_to(repo_root)
                        else str(target_path)
                    )
                    issues.append(LinkIssue(
                        str(md_path.relative_to(repo_root)),
                        lineno,
                        raw,
                        f'anchor #{anchor!r} not found in {rel_target}',
                    ))

    return issues


# ---------------------------------------------------------------------------
# Repo walk
# ---------------------------------------------------------------------------

def find_markdown_files(repo_root: Path) -> list[Path]:
    """Return all *.md files under *repo_root*, excluding common build dirs."""
    skip_dirs = {'.git', 'node_modules', 'target', 'dist', '.venv'}
    results: list[Path] = []
    for root, dirs, files in os.walk(repo_root):
        dirs[:] = [d for d in dirs if d not in skip_dirs]
        for f in files:
            if f.endswith('.md'):
                results.append(Path(root) / f)
    return sorted(results)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def run(repo_root: Path) -> int:
    if not repo_root.is_dir():
        print(f'error: {repo_root} is not a directory', file=sys.stderr)
        return 3

    md_files = find_markdown_files(repo_root)
    slug_cache: dict[Path, set[str]] = {}
    all_issues: list[LinkIssue] = []

    for md_path in md_files:
        all_issues.extend(check_file(md_path, repo_root, slug_cache))

    if not all_issues:
        print(f'✓ checked {len(md_files)} Markdown file(s): all links resolve')
        return 0

    for issue in all_issues:
        print(f'{issue.source_file}:{issue.line_number}: {issue.reason}  [{issue.raw_target}]')

    print(
        f'\n{len(all_issues)} broken link(s) found in '
        f'{len(md_files)} Markdown file(s)',
        file=sys.stderr,
    )
    return 1


def _repo_root_default() -> Path:
    """Two levels up from this script (.github/scripts/ → repo root)."""
    return Path(__file__).resolve().parent.parent.parent


if __name__ == '__main__':
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else _repo_root_default()
    sys.exit(run(root))
