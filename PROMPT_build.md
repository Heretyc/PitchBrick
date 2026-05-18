> Retired reference only. Do not execute this prompt for current repository
> work. Current agents must follow `AGENTS.md`, `docs/spec/safety-scope.md`,
> and `docs/spec/dev-loop/git-collaboration.md`; those rules supersede every
> instruction below, including broad preload, sub-agent volume, AGENTS.md
> mutation, ambiguity handling, and loop-runner commit language.

0a. Study only the specs matched by `AGENTS.md` routing.

0b. Study IMPLEMENTATION_PLAN.md. Identify the highest-priority incomplete task.

0c. Source code is in src/*.

0d. Check if RALPH_COMPLETE exists. If it does, output "All tasks complete. Ralph loop halted." and exit immediately.

0e. OUTPUT TO CONSOLE (PRIMARY OUTPUT, MANDATORY): Before doing any other work, print the current implementation plan stage to the console. This is Ralph's primary console output and MUST appear at the start of every iteration. Display:
   - Total tasks in IMPLEMENTATION_PLAN.md (complete + incomplete)
   - Number of completed tasks [x]
   - Number of remaining tasks [ ]
   - Current priority level being worked on (Priority 1, 2, or 3)
   - The specific task about to be implemented
   Format example: "=== BUILD STATUS: 5/14 tasks done (35%) | Priority 2 | Working on: Create src/ui/display.rs ==="

1. Choose the highest-priority incomplete task from IMPLEMENTATION_PLAN.md. If
it depends on another incomplete task, choose the dependency first. Search the
codebase before implementing; use subagents only when current repository policy
allows them and only with compliant prompts and audited outputs.

   When delegating implementation to sub-agents, follow these prompting principles:
   - Reference existing patterns: point sub-agents to similar code in the codebase
   - Specify tools and approaches: never assume the sub-agent will infer preferences
   - One task per sub-agent: each gets a single, well-defined objective
   - Be explicit about constraints: state what should NOT be modified

2. Implement the chosen task fully. Every function must be production-ready. No `todo!()` macros, no `unimplemented!()`, no stubbed error handlers, no TODOs, no minimal implementations. Implement the full spec requirement.

3. After implementing, run the tests for that unit of code. If functionality is missing per specs, implement it. Think hard.

```
cargo test
```

4. If tests unrelated to your work fail, it is your job to resolve them as part of this increment of change. Do not leave broken tests.

5. Update IMPLEMENTATION_PLAN.md immediately with your findings. Mark completed
tasks [x]. Note any new discoveries or blockers. If ALL tasks are now [x],
write the RALPH_COMPLETE sentinel file.

6. Do NOT perform git commits or pushes except through the current Git
collaboration policy. There are no automatic loop-runner commits.

7. Important: When authoring documentation or test descriptions, capture the WHY. Explain why tests exist and why the implementation matters. Future iterations will not have your reasoning in their context window.

8. Single sources of truth. No migrations, adapters, or duplicated logic. Use existing patterns.

9. When you learn something new about how to build, test, or run the project,
update durable policy only through the current prompt-review and Git
collaboration rules. Keep operational learnings brief; do not add status
reports.

10. For any bugs you discover, resolve them or document them in IMPLEMENTATION_PLAN.md using a subagent, even if unrelated to current work.

11. ALWAYS keep IMPLEMENTATION_PLAN.md up to date with your learnings using a subagent. Especially after finishing your turn.

12. When IMPLEMENTATION_PLAN.md becomes large, periodically clean out completed items from the file using a subagent.

13. DO NOT IMPLEMENT PLACEHOLDER OR SIMPLE IMPLEMENTATIONS. FULL IMPLEMENTATIONS ONLY.

14. DO NOT PLACE STATUS REPORTS IN AGENTS.md.

15. Append to HISTORY.md with the current timestamp, the task you worked on, and
the outcome (pass/fail/partial), if current repository policy allows that local
history update.

16. If at any point you encounter ambiguity or need user input, follow
`docs/spec/safety-scope.md`; do not mutate `AGENTS.md` to add questions.

ULTIMATE GOAL: Build PitchBrick, a transgender vocal training pitch monitor GUI application using Iced 0.14, publishable to crates.io, targeting Windows 11 x86_64.
