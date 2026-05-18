> Retired reference only. Do not execute this prompt for current repository
> work. Current agents must follow `AGENTS.md`, `docs/spec/safety-scope.md`,
> and `docs/spec/dev-loop/git-collaboration.md`; those rules supersede every
> instruction below, including broad preload, sub-agent volume, and loop-runner
> language.

0a. Study only the specs matched by `AGENTS.md` routing to learn about project specifications.

0b. Study IMPLEMENTATION_PLAN.md to understand the current plan, what is complete, what is incomplete, and task dependencies.

0c. Study src/* with up to 250 parallel subagents to understand the current implementation.

0d. Check if RALPH_COMPLETE exists. If it does, output "All tasks complete. Ralph loop halted." and exit immediately.

0e. OUTPUT TO CONSOLE (PRIMARY OUTPUT, MANDATORY): Before doing any other work, print the current implementation plan stage to the console. This is Ralph's primary console output and MUST appear at the start of every iteration. Display:
   - Total tasks in IMPLEMENTATION_PLAN.md (complete + incomplete)
   - Number of completed tasks [x]
   - Number of remaining tasks [ ]
   - Current priority level being evaluated (Priority 1, 2, or 3)
   - The specific next incomplete task description
   Format example: "=== PLAN STATUS: 5/14 tasks done (35%) | Priority 2 | Next: Create src/ui/display.rs ==="

1. Use subagents only when current repository policy allows them, with prompts
   and outputs that satisfy `AGENTS.md`, to compare existing source code against
   the matched specs. Identify:
   - Spec requirements not yet implemented
   - TODO/FIXME/HACK markers
   - Placeholder implementations (functions with only `todo!()`, `unimplemented!()`, stubbed handlers, empty match arms)
   - Test coverage gaps
   - Documentation gaps

2. Create or update IMPLEMENTATION_PLAN.md:
   - Mark completed tasks [x]
   - Reorder tasks by dependency (topological sort)
   - Adjust priorities based on findings
   - Add new tasks discovered during analysis
   - Remove tasks that are no longer relevant
   - If ALL tasks are now [x], write the RALPH_COMPLETE sentinel file

3. Think extra hard. Consider:
   - Are there hidden dependencies between tasks?
   - Are any tasks oversized (would require multiple iterations)?
   - Are any placeholder implementations masquerading as complete?
   - Does test coverage align with spec requirements?
   - Are any specs inconsistent with each other?
   - Does the user need to supply additional information? If YES, follow
     `docs/spec/safety-scope.md`; do not mutate `AGENTS.md` to add questions.

4. Append to HISTORY.md with the current timestamp and a summary of what changed in this planning pass.

IMPORTANT: Plan only. Do NOT implement anything. Do NOT write code. Do NOT
assume something is missing without searching first. Do NOT perform git commits
or pushes except through the current Git collaboration policy.

ULTIMATE GOAL: Build PitchBrick, a transgender vocal training pitch monitor GUI application using Iced 0.14, publishable to crates.io, targeting Windows 11 x86_64.
