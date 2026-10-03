# Techniques Not Used Here (and why)

The source approach ("How to Build AI Agents That Actually Learn From Their Mistakes") covers three techniques. This skill implements only the first; the other two are out of scope for a coding agent like Cline.

## 1. Reflection — implemented (this skill)

Agent writes a lesson after each objectively-verified failure and reads the store before the next attempt. Zero GPUs, session-durable, best for tasks with clear pass/fail signals (tests, builds) — exactly a coding agent's domain. Everything else in `SKILL.md` is this technique.

## 2. Reinforcement Learning — not applicable

Updating model weights from a reward function over thousands of trajectories.

- Requires ~1,000–5,000 collected trajectories and GPU training (TRL/OpenRLHF/Unsloth). We cannot generate that from one repo's tasks.
- Reward hacking risk: agents optimize the proxy, not the outcome ("tests pass" reward gets gamed by deleting tests).
- We don't own the base model's weights — Cline uses hosted/local third-party models.
- Wrong choice whenever the task is already solved by reflection (the article's own warning: don't build RL infrastructure when reflection solves 80% in two days).

## 3. Self-Play — not applicable

Two agents compete (attacker vs defender) and both improve. Useful for red-teaming and negotiation domains where the "judge" is the opponent. It needs an adversarial task generator and, for durable gains, RL on top. A desktop-lyrics-overlay repo has no adversarial structure to exploit; there is no meaningful attacker/defender split in "make the overlay render correctly."

## Ceiling of reflection (known limitation)

- Lessons are session/repo-durable only; they don't transfer to other users or repos.
- The value depends on the evaluator: if tests are weak, lessons are weak.
- Store grows unbounded unless stale lessons are reaped — hence the `reap` action.
