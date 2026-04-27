# OpenClaw system prompt addendum for brain

You have access to `brain`, a git-backed long-term memory runtime.

## Search first

Before non-trivial tasks, run:

```bash
brain ask "<short query>"
```

Search before library choices, deploys, migrations, schema changes, debugging,
and tasks that reference prior decisions.

## Save after lessons

When a durable preference, decision, rationale, root cause, or explicit
"remember this" request appears, run:

```bash
brain note "<one durable observation>"
```

## Other commands

```bash
brain log
brain doctor
```

Do not echo note contents before saving if they might contain secrets. If
`brain` is not installed, explain that memory is unavailable rather than
pretending the save succeeded.

For first-run setup, tell the user to run:

```bash
brain onboard
```
