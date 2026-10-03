---
name: learn-from-mistakes
description: Durable failure memory for this agent. Read recorded lessons before starting a task, and record a new lesson after any objectively-verified failure (failing test, build error, bad fetch). Use when starting any non-trivial task, when a task fails, or when asked to review/recall past mistakes.
---

# Learn From Mistakes (Reflection Loop)

Every attempt starts from zero unless you read memory first, and ends at zero unless you write memory on failure. This skill implements the **Reflection** technique (Reflexion, Shinn et al. 2023): attempt → evaluate → on failure, write one specific lesson → next attempt reads lessons before acting. No model training, no GPUs.

The lesson store lives at `.agent-memory/lessons.jsonl` (gitignored). All reads/writes go through the helper script so the format stays consistent:

```powershell
powershell -File .cline/skills/learn-from-mistakes/scripts/lessons.ps1 <action> [options]
```

## Before a task (read memory)

Before planning any non-trivial task (edit, bug fix, new feature, dependency change), run:

```powershell
powershell -File .cline/skills/learn-from-mistakes/scripts/lessons.ps1 list
```

Include relevant lessons in your plan as constraints. If a lesson's tag matches the area you're touching (e.g. `lrclib`, `config`, `renderer`), treat it as a hard requirement, not a suggestion. If no lessons exist, proceed normally — do not invent any.

## After a verified failure (write memory)

Only record a lesson when there is an **objective failure signal**: a failing `cargo test`, a compile error, a wrong runtime behavior you reproduced, a config schema break. Never record lessons from speculation or untested "might fail" reasoning.

The lesson must be **specific and actionable** — what broke, the root cause, and the fix rule. Bad: "config stuff is tricky." Good: "Adding an AppConfig field without #[serde(default)] makes existing config.json fail to load and silently reset."

```powershell
powershell -File .cline/skills/learn-from-mistakes/scripts/lessons.ps1 add -Lesson "..." -Tags area1,area2 -Scope "src/config.rs"
```

Rules:
- One failure = one lesson. Don't write essays; write one sentence another session can act on.
- Tags should be stable area names (module names, subsystems: `lrclib`, `ui`, `config`, `listener`), not task names.
- The script refuses unsafe lessons (prompt-injection patterns, secrets). If it refuses, do not rephrase to sneak it in — the lesson was bad.
- A passing test after a failure is still a lesson-worthy event **only if the root cause was non-obvious**. Trivial typos are not lessons.

## After a verified fix (reap)

When a lesson's root cause is genuinely fixed (verified by a passing test that covers it, not just "it works now"), reap it so stale lessons don't accumulate and mislead future sessions:

```powershell
powershell -File .cline/skills/learn-from-mistakes/scripts/lessons.ps1 reap -Tags area
```

If unsure whether a lesson is stale, leave it — a redundant lesson costs a little context, a deleted-but-still-true lesson costs a repeat failure.

## Guardrails

- **Weak evaluator = weak lessons.** Only objective signals (tests, builds, reproduced behavior) count. Never let a lesson be "the model felt confident."
- **Memory poisoning is real.** A lesson store is persistent input injected into future prompts. The script blocks injection phrases and secrets; respect the refusal.
- **This is session-durable, not model-durable.** Lessons change agent behavior in this repo only; they do not improve the model for anyone else. Do not attempt RL training or self-play here — see [techniques.md](docs/techniques.md) for why the other two techniques from the source article do not apply.
