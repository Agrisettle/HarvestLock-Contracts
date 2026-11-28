// Commitlint configuration for the CI Conventional Commits gate.
//
// We extend @commitlint/config-conventional and relax only the two rules
// that generate noise for documentation commits: body and footer lines are
// allowed to exceed 100 characters (long URLs, bullet summaries, and
// sentence-length prose are common in bodies and are not worth blocking CI).
//
// Everything else stays strict: header format/type-enum, subject case,
// empty-commit checks, etc. A PR with a non-conventional message still fails.
// `ignores` lists contributor commits that reached main inside an already
// reviewed PR (#314, #334, #337). Rewriting them would change their hashes and un-merge
// that PR, so they are exempted by exact header. Do not add new entries for
// open PRs; ask the author to reword instead.
const MERGED_HISTORY = new Set([
  'Merge upstream/main and finalize issues #303-#306',
  'Address upstream issues #303-#306',
  "feat(escrow): state machine model (#209), emit events (#212), replace_oracle_config (#215), event sourcing tests (#216)",
  "feat/docs: multi-asset spike, reassign_operator, fund/claim events, delivery deadline guard",
]);

module.exports = {
  extends: ['@commitlint/config-conventional'],
  ignores: [(message) => MERGED_HISTORY.has(message.split('\n')[0].trim())],
  rules: {
    'body-max-line-length': [0], // body lines may exceed 100 chars
    'footer-max-line-length': [0], // footer lines (BREAKING CHANGE, refs) too
  },
};
