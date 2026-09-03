# Spec gate round 9 — PRD-260902-0301-01. **PASS.**

> One `cold-reader` (`model: opus`, effort xhigh), 2026-09-02, per `SPEC-GATE.md` § Cold-reader
> procedure. Working tree at branch `feature/tmux-panes`, HEAD `70a878b`, artifacts uncommitted.
> Every finding recorded here was independently re-derived by the gate runner before being acted on.
> Rounds 7 and 8 are in the file beside this one; round 6 in the file beside that.

## Outcome

**Zero blocking findings.** Three documentation findings, all applied. Frontmatter now carries
`gate: passed 2026-09-02` and the void blockquote is deleted, per its own terms.

The round was gated against the exit condition the maintainer adopted after round 8, which the
deleted blockquote carried and which is recorded here so it survives:

> A finding blocks only if it changes an implementer's allowed behavior, the ownership of a slice, or
> an acceptance criterion. Prose that is merely stale or imprecise is corrected as documentation and
> does not fail the round.

Sets were judged against § Implementation Decisions, "How to read a set named in this document": a
current-tree population asserted as authoritative is a defect whatever its count, and a count that
happens to be right does not make it correct.

## Findings

| # | Dim | Class | Finding | Applied as |
|---|---|---|---|---|
| F1 | 3 | documentation | The body and the scan row both say the ADR mints **four** terms; the ADR's `terms:` and `CONTEXT.md` § Testing carry **five**. `Performance Check`, minted in the same session that wrote decision 5, is missing from both sentences — while the PRD's own frontmatter `terms:` lists it. Non-blocking: the row's `decided` verdict still resolves (all five entries exist), and no acceptance criterion is derived from the count. | Body sentence and scan-row citation now say five and name `Performance Check` |
| F2 | 4 | documentation | "**Production changes are in scope where they make behavior observable.** Two are decided" closes a **decision**-kind set at two where three are decided. The omitted one is the run-lifecycle entry point handing back the receiver or accepting a pre-registered run id — decided in the imperative under "The database stays real; the polling goes", and quantified there over *every* run-lifecycle entry point that spawns. Non-blocking: `-05` cites that section rather than the summary list, so no slice ownership and no AC moves. | Third bullet added, pointing at the section that decides it and its reach |
| F3 | 9 | documentation | "the sibling helper **one line below**" — `tmux_available` and `tmux_session_exists` are four lines apart. The structural claim around it is exact. | Rewritten symbolically as "the sibling `tmux_session_exists`", per the positional-citation convention |

## Closed-set sweep

Mechanical extraction after masking artifact IDs and inline code spans, then falsification of each
survivor against the tree with an exact command.

| | |
|---|---|
| Sentences extracted | 359 |
| Carrying a quantifier or numeral | 224 |
| Asserting a population (the ledger) | **105** |
| Defective | **3** |

The three defects are F1 (two sites) and F2. **Everything the PRD decides survived falsification** —
the tier rule, the seam constraints, the selector constraints, the acceptance recipe, the
vacuous-skip genus, the clock-identity quantifier, and every `file:line` source citation in the
document resolved to the named construct.

Round 8's six findings were each re-derived against the current file and each is applied: the
per-test exception derivation with the category-label prohibition (F1/F2), the vacuous-skip genus
(F3), the open remaining-wall-clock set (F4), the three-way-aligned clock-identity quantifier (F5),
and the split fixture referent (F6). F7 and F8 are gone with the scan row and the blockquote.

## Scan-row verification

Every owed row verified against the text at its citation: problem/success, scope boundary, domain
terms, architecture shape, stack, data/schema, contracts/integrations, testing/seams, ops envelope
all `decided` and all resolving. `UX` and `NFRs` `n/a` with reasons that hold — the cold reader
tested both against body text and found no position taken on either (the `Performance Check`
category and the frontend timeout floor are tiering and test-config decisions under dimension 9,
not a product bar). Zero `[ASSUMED:]`, zero `[OPEN:]`, zero `open` rows; `terms:`/`adrs:`/`issues:`
reciprocity holds in both directions.

## What the stamp does not claim

The three documentation corrections above were applied **after** the pass and were not re-gated.
Each is a numeric or pointer correction to prose whose surrounding claim the round verified; none
changes a decision, an ownership, or an acceptance criterion. Recorded rather than re-gated, per the
exit condition.

One gap the round surfaced is **not** repaired here and is not a defect in this document:
`CONTEXT.md:5` makes the epic that establishes a practice the owner of removing the
`_(planned — ADR-…)_` marker from the terms its ADR names, and no slice of this epic owns that
removal for the five Testing entries. Routed to the maintainer as a decomposition decision.
