---
id: ISSUE-260905-2136-02
kind: issue
category: bug
status: needs-triage
summary: Driver capability tests assert fixed values while the production loader reads the developer's own tmux-tools agents.toml through a process-global cache, so results depend on the machine the suite runs on
---

## Triage Notes

Filed 2026-09-05, from the adversarial review of `PRD-260902-0301-01` (Codex, `gpt-6-astra`).
Reported here rather than folded into the narrowed epic: no existing slice owns registry input, and
the epic was scoped to the reproduced failure plus the tier rule.

**The claim, not yet verified by me.** `claude_capabilities` and `codex_capabilities`
(`src/driver.rs:111`, `:155`) assert fixed capability values, but the production driver implementations
resolve capabilities through the agent registry, which fingerprints
`$XDG_CONFIG_HOME/tmux-tools/agents.toml` or `$HOME/.config/tmux-tools/agents.toml` and caches the
result in a process-global static (`src/driver.rs:727`, `:1837`). A second witness cited was
`registry_profile_driver_capabilities_default_when_absent`, which assumes its chosen profile name is
absent from whatever registry happens to be on the machine.

I have not confirmed these call paths, and no user configuration was modified to reproduce a failure.
Triage should start by confirming the loader is actually reached from those tests.

**Discovery.**

```
rg -n 'agents\.toml|XDG_CONFIG_HOME|registry' src/driver.rs
cargo test --locked driver:: 2>&1 | tail -40     # then re-run with a populated agents.toml
```

The decisive check is whether creating an `agents.toml` entry for the profile names these tests use
changes their outcome.

**Why it belongs to the tier work conceptually, and still not to this epic.** A test whose result
depends on the developer's dotfiles is not isolated, which is the Logic Tier's whole criterion — and
like `ISSUE-260905-2136-01`, no scan of test bodies can see it. But it is ambient *configuration*
rather than time, it did not cause the reproduced failure, and `ISSUE-260902-0747-14` narrowly
removes login-shell resolution from struct-field tests without touching the registry.

**Shape of the fix, for triage to confirm.** Prefer supplying an explicitly constructed registry with
an owned cache over mutating process-global environment variables, which is unsound under parallel
tests. Precedent for explicit per-test registry construction already exists at `src/driver.rs:1935`,
`:1949`.

**Related.** `ISSUE-260905-2136-01` (sibling: hidden production input defeating a syntactic checker),
`ISSUE-260902-0747-04` (the other suite-wide test-honesty defect: absent dependency reads as pass).
