---
id: ISSUE-260905-2136-03
kind: issue
category: bug
status: needs-triage
summary: Two pane-stream timeout tests sequence work with multi-second shell delays and assert a negative observation across a fixed window, so they can pass early against a broken implementation; this is the largest itemised block of retained Integration timing debt
---

## Triage Notes

Filed 2026-09-05, from the adversarial review of `PRD-260902-0301-01` (Codex, `gpt-6-astra`).
This record is the named owner for part of the retained timing debt the ADR now itemises
(`docs/adr/260902-0312-deterministic-test-tiers.md` § Retained debt).

**The claim, not yet verified by me.** `timed_out_pane_stream_enable_cannot_replace_a_newer_pipe`
(`src/api.rs:4403`) and `timed_out_pane_stream_stop_cannot_run_after_owner_exits` (`src/api.rs:4445`)
use shell delays of seven and eight seconds, then sleep a further two seconds before checking the log.

Two separate problems were reported:

1. They sequence work with sleeps, which the ADR forbids for tests written from now on. As existing
   tests they are retained debt, not violations, until this record drains them.
2. The negative observation is unsound in principle: asserting that nothing happened within a
   two-second window does not establish that a surviving descendant cannot run *after* it. A longer
   sleep does not fix this — no sleep length makes a negative observation a proof.

The second point is the one worth acting on, and it is a correctness argument about the assertion
rather than a flakiness argument. Neither was reproduced against a deliberately broken implementation.

**Why no existing slice owns them.** `ISSUE-260902-0747-02` explicitly preserves fixture script
behavior; `ISSUE-260902-0747-08` is scoped to interactive agent poll loops. Both were deferred out of
the narrowed epic in any case.

**Also in this family, separate mechanism.**
`registry_cache_reloads_after_equal_length_rewrite_with_restored_mtime` (`src/driver.rs:1190`) sleeps
10ms between equal-length writes and restores mtime, depending on the filesystem fingerprint changing
via ctime and inode (`src/driver.rs:166`). Guessing a sleep does not deterministically measure ctime
resolution. Triage should consider splitting cache policy with supplied fingerprints from the
filesystem-fingerprint behavior, so the policy half becomes Logic Tier and only the filesystem half
stays real.

**Discovery.**

```
rg -n 'sleep|Duration::from_secs' src/api.rs src/driver.rs | rg -n 'test|async fn'
```

**Related.** `PRD-260902-0301-01` (narrowed epic that does not own these),
`ISSUE-260902-0747-02` and `-08` (deferred slices that touch the same files).
