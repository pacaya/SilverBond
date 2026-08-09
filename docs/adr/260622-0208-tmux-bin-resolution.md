---
id: ADR-260622-0208-01
status: accepted
origin: docs/decisions/tmux-bin-resolution.md
---

# Tmux Binary Resolution Uses Interactive Zsh

Status: Accepted
Date: 2026-06-21

SilverBond resolves the tmux binary for `runAs` invocations with `zsh -lic`.
The `-i` flag is intentional: zsh only sources `.zshrc` for interactive shells,
and operator tooling paths for Homebrew, asdf, pyenv, nvm, and similar tools
commonly live there. Replacing this with login-only `zsh -lc` can make tmux
resolution fail for valid operator environments.

Because interactive rc files can write banners to stdout, the lookup command
prints a sentinel-prefixed line and the resolver only accepts that line. Empty
sentinel values fall back to `tmux`, and the subprocess runs through the shared
tmux-tools-core bounded-wait helper so a blocking rc file cannot park a worker
thread indefinitely.
