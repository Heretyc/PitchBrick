# PitchBrick Commit Style Guide

## Prefix System (Mandatory)

Every commit message begins with one or more title-cased action prefixes followed by a colon and a space.

| Prefix | Meaning |
|--------|---------|
| `Added:` | New features, new files, new capabilities |
| `Fixed:` | Bug fixes, corrections, reworked broken behavior |
| `Updated:` | Modifications to existing files/dependencies (nothing broken) |
| `Moved:` | Relocated files or documentation |
| `Changed:` | Renamed or altered existing behavior without it being a fix |
| `Removed:` | Deleted code, files, or dependencies |

## Rules

- This guide is subordinate to `docs/spec/dev-loop/git-collaboration.md`.
  When repository policy requires motivation, risk, migration, rollback, or
  review context, include a normal commit body even if the subject still follows
  this prefix style.
- Prefix is ALWAYS title-cased: `Added:`, not `added:` or `ADDED:`
- Colon immediately after the prefix word, followed by exactly one space
- Description starts with a capital letter
- No trailing period
- No articles when possible ("Added: Docker support" not "Added: a Docker support feature")
- NEVER add ANY AI or Agentic workflow (Such as Claude or Codex) as a co-author on any commit for any reason. The ONLY permitted author on the commit is the human user.

## Multi-Line Format

Each change gets its own line. No blank lines between entries. Group by prefix type in priority order: Added > Fixed > Updated > Moved > Changed > Removed.

```
Added: Audio capture module
Added: FFT frequency analyzer
Updated: Cargo.toml dependencies
```

## What NOT to Do

- No conventional commits (feat:, fix:, chore:)
- No decorative blank-line body sections when policy does not require a body
- No trailing periods
- No lowercase or ALL CAPS prefixes
- No dash prefixes (- Added:)
- No Co-Authored-By lines
- No issue/PR references
- No generic descriptions ("various improvements")
- No emoji
- No invented prefixes (Refactored:, Improved:, Created:)
- No markdown formatting in commit messages

## Special Commits

- Version-only releases: just the version string, e.g., `v1.1.8`
- Initial commits: `Initial Commit` or `Initial commit`
