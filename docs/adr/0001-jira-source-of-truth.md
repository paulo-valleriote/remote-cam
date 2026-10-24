# ADR-0001 — Jira is the source of truth; the speckit flow is removed

- **Status:** accepted
- **Date:** 2026-10-24
- **Supersedes:** the speckit planning flow in `.specify/` and `.opencode/commands/speckit.*`

## Context

The repository carried two planning systems: the Jira board `REC` and a local
speckit installation (`.specify/` plus ten `/speckit.*` commands). The F0 record
under `specs/001-f0-technical-spikes/` was produced through speckit, but every
open issue and every decision since has lived in Jira. Keeping both means two
places can disagree about what is planned and what is done, and the speckit
templates keep generating artifacts that Jira already owns.

The project constitution also lived in `.specify/memory/constitution.md`, which
made a governing document depend on the tool being retired.

## Decision

- Jira board `REC` is the single source of truth for issues, priorities and
  acceptance criteria. Agents read and write it through the Atlassian MCP
  (see `docs/agents/issue-tracker.md`).
- The speckit flow is removed: `.specify/` and all ten `speckit.*` commands are
  deleted. No `AGENTS.md` refers to either.
- `specs/001-f0-technical-spikes/` is kept unchanged as the immutable F0
  record. It is evidence of how F0 was specified, not a live workflow.
- The constitution moves to `docs/constitution.md` and is referenced from the
  root `AGENTS.md`. Durable decisions live in `docs/adr/`; shared vocabulary
  lives in `GLOSSARY.md`.

## Consequences

- New work starts as a Jira issue; there is no `/speckit.specify` or
  `/speckit.plan` step to run.
- Planning artifacts that must survive a change become ADRs or glossary
  entries, not generated spec folders.
- `specs/` receives no new feature directories. A future decision to reopen it
  needs a new ADR that supersedes this one.
