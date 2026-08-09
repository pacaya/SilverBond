---
id: ADR-260809-1601-01
status: accepted
---

# Adopt the shared record conventions for issues, decisions, and reviews

SilverBond's in-repo records grew ad hoc: issue IDs used an unpadded batch ordinal, and the
one architectural decision sat in a `docs/decisions/` directory belonging to neither the flat
nor the graduated docs layout. This repo now follows the shared conventions — `<KIND>-YYMMDD-HHMM-NN`
IDs with a zero-padded ordinal, ADRs in `docs/adr/` with an `INDEX.md`, issues in `docs/issues/`
(+ `done/`), and code reviews in `docs/issues/code-reviews/` — so that an agent arriving with no
prior context can locate and cite any record from the conventions alone.

## Consequences

**Migrated.** Two issue IDs gained the mandatory zero-padded ordinal, renaming both record files and
rewriting all 28 in-corpus citations across six files:

- `ISSUE-260723-0823-1` → `ISSUE-260723-0823-01`
- `ISSUE-260722-0818-1` → `ISSUE-260722-0818-01`

  Provenance: `migrate-id-grammar.sh --tier datetime --apply` (2 renames, 6 content files), plus a
  judgment pass over the 9 report rows it bucketed `mapped-but-quoted`; 2026-08-09.

`docs/decisions/tmux-bin-resolution.md` moved to `docs/adr/260622-0208-tmux-bin-resolution.md` and
gained `id: ADR-260622-0208-01` / `status: accepted`. The datetime is the file's git first-add in UTC,
not the `Date: 2026-06-21` its body carries — those differ by one calendar day because 02:08 UTC on
the 22nd is the evening of the 21st in a western zone. Its decision text is unchanged.

  Provenance: `TZ=UTC git log --diff-filter=A --follow --format=%ad --date=format-local:%y%m%d-%H%M --
  docs/decisions/tmux-bin-resolution.md` → `260622-0208` (commit `482ba08`); 2026-08-09.

**Grandfathered — two shapes coexist deliberately.**

Six code-review filenames predate the `<branch>-code-review-<TS>.md` grammar and keep their existing
names: `code-review-20260608_part_{1..4}_of_4.md`, `feature-tmux-panes-code-review-20260608.md`, and
`feature-tmux-panes-code-review-20260610.md`. The grammar's `<TS>` is compact UTC `YYYYMMDD-HHMMSS`,
and the seconds exist nowhere: unlike every conforming sibling these files carry no `**Generated:**`
header line, and git cannot supply the value either — all four `_part_` files share one first-add
commit (`081e591`, `20260608-061746`) and both `feature-tmux-panes-*` files share another (`aa7a0a2`,
`20260621-195207`) with a third, already-conforming review file. Deriving from git would collide two
filenames, collide with `feature-tmux-panes-code-review-20260620-092605.md`, and move 8-June and
10-June reviews to 21 June — destroying the chronology the timestamp exists to record. Fabricating
seconds was rejected. Renaming would additionally strand roughly fifteen basename citations inside
four finalized review reports.

`docs/tasks/task-L2-nodekind-tagged-enum-refactor.md` stays where it is: `scripts/check-canonical-v4-docs.sh`
reads that path as an exclusion, and converting a superseded plan document into an issue record would
require inventing frontmatter it never had.

**Not created.** No `CONTEXT.md`, `SPINE.md`, PRD, roadmap, decision map, or `.out-of-scope/` entry —
none existed, and this migration authors no record that did not already exist. This ADR is the sole
exception.
