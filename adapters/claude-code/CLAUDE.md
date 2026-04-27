# Claude Code instructions for brain

This file is an optional project-local version of `brain.md`. Use
`brain onboard --agents claude-code --yes` for normal setups.

## Memory behavior

- Search with `brain.ask` before non-trivial work.
- Save durable lessons with `brain.note` after preferences, decisions,
  root-cause discoveries, and explicit "remember this" requests.
- Use `brain.log` when the user asks what has been saved recently.
- Run `brain.doctor` if memory results look inconsistent.

## Safety

- Do not echo rejected note text back to the terminal.
- Do not save secrets, credentials, private keys, bearer tokens, or API keys.
- If `brain` is unavailable, explain that the runtime is not configured rather
  than pretending memory was saved.
