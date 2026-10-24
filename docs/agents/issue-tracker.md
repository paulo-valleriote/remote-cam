# Issue tracker: Jira (Atlassian MCP)

Issues and specs for this repo live in Jira, accessed via the Atlassian MCP (`tools.atlassian`). Board: REC. Columns: to do, in progress, done.

## Conventions

- **Create an issue**: Use `tools.atlassian` operations (`executeWrite` with appropriate operation name, e.g., `createJiraIssue`).
- **Read an issue**: Use `tools.atlassian` read operations (`executeRead` with `getJiraIssue`).
- **List issues**: Use `tools.atlassian` search operations (`executeRead` with `searchJiraIssuesUsingJql`).
- **Transition an issue**: Use `tools.atlassian` executeWrite with `transitionJiraIssue`.
- **Comment on an issue**: Use `tools.atlassian` executeWrite with `addCommentToJiraIssue`.
- **Apply / remove labels**: Use `tools.atlassian` executeWrite with `editJiraIssue` (labels field).
- **Close / Done**: Transition to "done" column via `transitionJiraIssue`.

## Pull requests as a triage surface

PRs as a request surface: no. (This repo does not treat external PRs as feature requests.)

## When a skill says "publish to the issue tracker"

Create a Jira issue on board REC.

## When a skill says "fetch the relevant ticket"

Read it using the Atlassian MCP read operations.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a single Jira issue with **child** issues as tickets.

- **Map**: a single Jira issue labelled `wayfinder:map`, holding the Notes / Decisions-so-far / Fog body.
- **Child ticket**: a Jira issue linked to the map (sub-task or "blocks" relation). Labels: `wayfinder:<type>` (`research`/`prototype`/`grilling`/`task`). Once claimed, assign to the driving dev.
- **Blocking**: Jira's native issue links (blocks / is blocked by). A ticket is unblocked when every blocker is resolved (in "done" column).
- **Frontier query**: list the map's open children, drop any with an open blocker or an assignee; first in map order wins.
- **Claim**: assign the issue to the current user (via `editJiraIssue` with assignee).
- **Resolve**: add comment with answer, transition to "done", then append a context pointer (gist + link) to the map's Decisions-so-far.