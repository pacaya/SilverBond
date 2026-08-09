---
id: ISSUE-260722-0818-01
kind: issue
category: bug
status: wontfix
summary: M16 infers privilege from canonical profile names
---

## Triage Notes

Escalated from code review.

**Source:** `feature-tmux-panes-code-review-20260720-032528.md` — finding `L19`

**Blocker (why it could not be fixed in the run):** The root-cause fix requires a first-class privilege declaration in the upstream `tmux-tools-core` crate (`AccessProfile` / `AccessProfileConfig`), plus a `Cargo.toml` rev-pin bump. `cargo metadata` confirms `tmux-tools-core` resolves only to a read-only git-checkout cache (`~/.cargo/git/checkouts/…`) with no vendored copy, path dependency, `.cargo/config.toml` override, or `[patch]` entry in this repo — matching the H8/M31 precedent that the crate is not editable/re-pinnable here. A partial in-repo hardening did land in the fix run (commit `a22abc1`): `resolve_registry_access_profile` now runs `require_privilege` + the access-mode ceiling check on the mode-mapped profile arm, closing the code-level asymmetry with the override arm. But `declared_profile_privilege` still derives privilege purely from the canonical profile name, so a deceptively-named broad-argv profile cannot be caught without the upstream metadata. Completing this needs upstream access.

---

**Triage resolution (2026-08-08) — `wontfix`, rejected: the behaviour is intended.**

*Claim verified.* The escalation reproduces, and by a simpler route than the finding describes.
The finding routes the attack through a custom-named profile plus the byte-identical shape match;
that detour is unnecessary. Upstream `merge_agent_config` (`core/src/agents/mod.rs:377`) inserts
operator profiles over builtins **by key**, so an operator can overwrite the canonical profile's
args directly. With `[codex.access.read-only] args = ["--sandbox", "danger-full-access"]` in
`agents.toml`, an `AccessMode::ReadOnly` run resolves profile `read-only` and launches
`["--sandbox", "danger-full-access"]`. Confirmed with a throwaway test against
`resolve_registry_access_profile`; not retained (see below).

*Rejected as a defect.* `~/.config/tmux-tools/agents.toml` is operator-owned, and the finding's own
severity note concedes the path crosses no trust boundary. Exploiting it requires the operator to
attack themselves — someone who can edit that file can already launch any agent with any flags
directly. An operator redefining their own profiles is configuration, not a vulnerability.

The governing semantics, now documented in `docs/agent-drivers.md` (Registry Profile Driver):
**the profile name is the operator's privilege declaration**, not a heuristic SilverBond infers
from. The finding's core complaint — that names cannot prove what free-form argv permits — assumes
SilverBond was trying to prove something. It is honouring a declaration. Two guarantees, both from
the `a22abc1` hardening, remain and are the actual contract: unranked names fail closed
(`require_privilege`, `src/driver.rs:334-340`), and a profile's declared rank can never exceed the
run's requested `access_mode` (`:396-402`). The operator may redefine what a rank *means*; they
cannot escape the rank they declared.

*The stated blocker was wrong, and is moot.* The record claimed `tmux-tools-core` is not
editable/re-pinnable because `cargo metadata` resolves it only to `~/.cargo/git/checkouts/`. That
is how cargo resolves every git dependency and is not evidence about editability — a writable,
user-owned clone exists at `/Users/Shared/Data/work/Programming/tools/tmux-tools` (remote
`pacaya/tmux-tools`, containing the pinned rev `69173d1`), running its own `docs/issues/` pipeline.
The H8/M31 precedent cited here does not hold. It no longer matters: the recommended upstream
`privilege: Option<Privilege>` field exists to let a profile declare a rank independently of its
name, and under the name-is-the-declaration doctrine there is nothing left for it to express. No
two-repo change, no rev pin bump.

*Rejected alternative.* An in-repo-only fix was available — `Registry::load_with_user_path(None)`
(`core/src/agents/mod.rs:172-194`) returns the builtin-only registry, so SilverBond could refuse to
rank any canonical profile whose args differ from builtin. Rejected because it breaks exactly the
operator customisation this resolution affirms: an operator adding a flag to `workspace-write`
would silently lose the ability to launch under `Edit`/`Execute`.

*No code change.* No regression test was added — there is no defect to pin, and a test asserting
the mislabeled profile is refused would encode the opposite of the decided semantics. The existing
`registry_edit_accepts_narrow_ranked_default_fallback` (`src/driver.rs:1971-1999`) and
`registry_edit_rejects_unranked_access_profile_override` stand unchanged; the latter is the
fail-closed guarantee above.

*Known exception, left in place.* The shape-matching fallback (`src/driver.rs:315-329`) grants a
rank to a non-canonically-named profile whose argv is byte-identical to a canonical sibling. It is
the one place SilverBond still infers rather than honours a declaration, making it the exception to
the documented doctrine. Left as-is per maintainer decision — harmless and convenient. Revisit only
if the doctrine needs to be exceptionless.

---

**Original finding body (verbatim):**

#### Issue L19: M16 infers privilege from canonical profile names
**Severity:** LOW
**File(s):** `src/driver.rs:291-296` (`declared_profile_privilege` name table), `:311-314` (comment), `:315-329` (shape-matching fallback), `:340`, `:356`, `:390-400`, `:405-407` (unguarded name-mapped return), `:414`, `:419` (gate), `:1971-1999` (existing fallback test); upstream `core/src/agents/mod.rs:29-31` (`AccessProfile`), `:264-268` (`AccessProfileConfig`), `:175-186`, `:335-350` (operator registry merge); `Cargo.toml:28` (rev pin)
**Description:** *(Severity note in report: LOW — reachable only by the operator mislabeling profiles in their own `agents.toml`, crossing no trust boundary; workflow authors cannot add or edit profiles, and the name-only path at `src/driver.rs:405-407` already grants the same escalation with less effort. Depends on M31, which wraps the four `ok_or_else` sites into `require_privilege` first.)* `declared_profile_privilege` (`src/driver.rs:291-296`) ranks `read-only`/`workspace-write`/`full-access` purely by string, and for non-canonical names the fallback at `:315-329` accepts a profile only if its argv is byte-identical to a canonically-named sibling. An operator registry containing `[custom.access.read-only] args=["--broad"]` plus an identical `default` therefore makes `declared_profile_privilege(spec, "default")` return `ReadOnly`, pass `default_privilege <= config.access_mode.privilege()` at `:419`, and launch broad argv under `Edit` — the widening M16 was filed to stop. The larger hole is on the primary path: `:405-407` returns the name-mapped profile with **no rank check at all**, so broad argv placed under `[custom.access.workspace-write]` launches under `Edit` without ever reaching `declared_profile_privilege`. Upstream `AccessProfile` carries `{ args: Vec<String> }` only (`core/src/agents/mod.rs:29-31`), so SilverBond currently has no channel through which a profile could declare its own privilege.

*Corrections to the original write-up:* M16 did prohibit name-keying (`:452`), but its stated reason was that such a map *"cannot rank `default` at all, which is the entire question"* — **not** L19's claim that names cannot prove what free-form argv permits. The shipped fix does not rank `default` by name; it shape-matches it, and M16's own fix header (`:431`) records this as the deliberate accepted compromise *"using exact local profile-shape validation because an upstream rev bump was not authorized."* The `access_profile_override` arm workflow authors can reach **is** rank-guarded (`:390-400`); they can select a profile name but cannot add or edit profiles.
**Recommended Fix:** Give profiles a first-class privilege declaration upstream and consume it, failing closed when it is absent.
- Add `privilege: Option<Privilege>` to `AccessProfile` (`core/src/agents/mod.rs:29-31`) and to `AccessProfileConfig` parsing (`:264-268`), declare it on all four builtin profile sets, and bump the `tmux-tools` rev pin at `Cargo.toml:28`. This is a coordinated two-repo change; the H6 precedent establishes that upstream edits are in scope.
- Delete the name table (`src/driver.rs:291-296`) and the shape-matching fallback (`:315-329`), resolving privilege solely from the declaration and treating a missing declaration as unranked so the existing `ok_or_else` bails at `:340`, `:356`, `:390`, `:414` fail closed.
- **Gate `:405-407` on the same declaration** — this is the load-bearing part of the change. Without it the finding is largely cosmetic, since that path grants the same escalation unconditionally. *(Note: the partial in-repo hardening in commit `a22abc1` added a `require_privilege` + ceiling check on this arm, but it is still name-derived until the upstream declaration exists.)*
- Tolerate `privilege: None` on operator-authored profiles as unranked rather than a parse error, so existing `agents.toml` files keep loading while losing only their ability to satisfy `Edit`/`Execute` gates.
- Correct the comment at `:311-314`, which currently describes rigor the resolution chain will no longer implement in the same terms.
- Add a regression with a deceptively named broad-argv profile (a `read-only` profile carrying wide args) asserting the launch is refused, and a companion asserting a mislabeled `workspace-write` profile is refused at `:405-407`. The existing positive test `registry_edit_accepts_narrow_ranked_default_fallback` (`:1971-1999`) exercises this exact path with honest argv and must be reworked onto the declaration.
