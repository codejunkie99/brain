# AGENTS.md — Hermes adapter for brain

This workspace uses `brain` for durable, git-backed memory.

## Memory behavior

- Before non-trivial work, search prior memory:
  ```bash
  brain ask "<short query>"
  ```
- After preferences, decisions, root causes, or explicit "remember this"
  requests, save one high-signal note:
  ```bash
  brain note "<one durable observation>"
  ```
- Use `brain log` for recent notes and `brain doctor` for health checks.

## Discipline

- Save signal, not every turn.
- Do not echo note contents before saving if they might contain secrets.
- If `brain` is not installed, say so and ask the user to run
  `brew install codejunkie99/tap/brain`.
