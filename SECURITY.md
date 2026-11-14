# Security Policy

## Supported versions

The project is pre-alpha. Only the `main` branch receives security fixes until 1.0.0 is released.

## Reporting a vulnerability

Please do not open a public issue for security problems. Use GitHub's private vulnerability
reporting on the repository (**Security → Report a vulnerability**). Include:

- the affected crate and version or commit,
- steps or a minimal image that reproduces the problem,
- the impact you observed (for example: a write to a source device, a panic on crafted input,
  an out-of-bounds read).

We aim to acknowledge reports within 7 days.

## Scope

In scope: memory-safety issues, panics or hangs on malformed images or filesystem structures,
any path that writes to a device that should be read-only, and incorrect "intact" verdicts from
the media validators that could mislead a recovery.

Out of scope: data loss caused by imaging hardware that is already failing. That is expected and
is what the mapfile exists to mitigate.
