<!--
Sync Impact Report
==================
Version change: 1.0.0 → 1.1.0
Ratification: 2026-10-21 (initial adoption)
Last amended: 2026-10-21
Modified principles:
  - I. Qualidade de Código → I. Code Quality (translated; documentation language rule expanded)
  - II. Testabilidade → II. Testability (translated)
  - III. Manutenibilidade e Observabilidade → III. Maintainability and Observability (translated)
Modified sections:
  - Estrutura de Pastas e Limites de Módulo → Project Structure and Module Boundaries
  - Fluxo de Desenvolvimento e Portões de Qualidade → Development Workflow and Quality Gates
  - Governance (translated)
Added rules:
  - All project documentation MUST be written in English (constitution, READMEs, docs/,
    specifications, plans, review artifacts)
Removed rules:
  - Previous allowance for documentation and governance artifacts to remain in Portuguese
Follow-up TODOs: none
-->

# Remotecam Constitution

## Core Principles

### I. Code Quality (NON-NEGOTIABLE)

All production code MUST be written in English, including variable names, functions, types, files,
and internal error messages. All project documentation MUST be written in English, including this
constitution, root and per-application READMEs, files under `docs/`, specifications, plans, and
review artifacts. No document MUST be authored in another language.

- Every module MUST be interface-first: the public contract is defined before implementation and
  MUST NOT expose details of the chosen technology.
- Every external dependency MUST be injected (dependency injection). Application logic MUST NOT
  import, instantiate, or know SDKs, HTTP clients, databases, drivers, or UI frameworks directly;
  those dependencies live behind adapters that implement domain interfaces.
- No external library MUST be added without an explicit justification recorded in review (problem
  solved, alternatives discarded, maintenance cost, and license). Without that justification, the
  dependency MUST be rejected.
- Names MUST be meaningful and complete. Abbreviations are forbidden, except for established
  technical terms (for example, `id`, `url`, `http`).
- Documentation is the code: comments MUST be avoided. Prefer expressive names, single
  responsibility functions, and clear contracts. Extensive, necessary documentation MUST live in
  `docs/`, never in comment blocks inside code.
- Accidental complexity MUST be justified in review. If a simpler solution meets the requirement,
  it MUST be preferred.

Rationale: readable, decoupled code with explicit contracts lowers the cost of change, allows
replacing technologies without rewriting business rules, and makes review objective. A single
documentation language removes translation drift and keeps the project accessible to every
contributor.

### II. Testability (NON-NEGOTIABLE)

- Test coverage MUST be greater than or equal to 90% in every delivered package. The coverage gate
  MUST be verified automatically before merge.
- Tests MUST NOT be deleted, disabled, or weakened to work around failures. The only exception is
  when the prompt or the specification explicitly authorizes removal.
- All domain logic MUST be validated by deterministic unit tests, without network access, real
  clock, or shared state between cases.
- Every user-facing feature MUST be validated by an integration test with external dependencies
  mocked or by an end-to-end test. Both success and error paths MUST be covered.
- Code with shared state or concurrent execution MUST have a concurrency test that exercises
  races, event ordering, and idempotency.
- Long-lived components (processes, streams, connections, caches, listeners, subscriptions) MUST
  have a memory or resource leak test that verifies release after shutdown.
- Tests MUST NOT use production data, credentials, secrets, or sensitive information. Fixtures
  MUST be synthetic and versioned alongside the test.
- Tests MUST be deterministic and independent of execution order.

Rationale: coverage without meaningful tests is vanity; deterministic, isolated, contract-oriented
tests are the only reliable way to evolve the product without regressions.

### III. Maintainability and Observability

- Every log line MUST describe exactly the event that occurred, with enough context for diagnosis
  (operation, correlation identifier, and outcome). Logs MUST be structured and consistent across
  desktop and mobile, MUST be written in English, and MUST NOT contain secrets or personal data.
- Every metric MUST have a declared purpose and a defined consumer (dashboard, alert, or decision)
  before it is added. Speculative metrics MUST be rejected.
- The root `README.md` MUST contain the instructions to install, run, test, and debug the project.
  `desktop/README.md` and `mobile/README.md` MUST contain the instructions specific to each
  application. Every change in behavior, command, or prerequisite MUST update the corresponding
  README in the same delivery.
- The root `Makefile` MUST remain up to date with targets for running, testing, coverage, linting,
  and debugging each application. Every command documented in a README MUST exist in the Makefile.
- Failure diagnosis MUST be possible using only the documented logs, metrics, and tests, without
  ad hoc instrumentation.

Rationale: maintained software is observable software. Purposeful logs and metrics, combined with
executable documentation, reduce diagnosis time and prevent knowledge loss.

## Project Structure and Module Boundaries

The repository root MUST follow the structure below. New first-level directories MUST be justified
in review.

```text
remotecam/
├── desktop/          # Desktop application code
│   ├── docs/         # Desktop-specific documentation
│   └── README.md     # Desktop execution instructions
├── mobile/           # Mobile application code
│   ├── docs/         # Mobile-specific documentation
│   └── README.md     # Mobile execution instructions
├── README.md         # Project execution instructions
└── .gitignore        # Files that must not be versioned
```

- `desktop/` and `mobile/` MUST be self-contained: neither MUST import source code from the other.
  Shared contracts MUST be extracted into a dedicated, versioned, and tested module, or exposed
  through a documented network interface.
- Platform-specific documentation MUST live in that platform's `docs/`; the README MUST remain an
  execution guide, not a repository for extensive documentation.
- `.gitignore` MUST cover secrets, environment files, build artifacts, installed dependencies,
  caches, and generated files. No secret MUST be versioned, even in fixtures or examples; examples
  MUST use fictional and explicitly invalid values.

## Development Workflow and Quality Gates

- Every change MUST go through review that explicitly verifies Principles I–III and the module
  boundaries defined in this constitution.
- Merge MUST be blocked when any test is failing, coverage is below 90%, lint reports an error, or
  execution documentation is outdated.
- Adding an external dependency MUST present a justification in the change description, as defined
  in Principle I.
- Deviations from any principle MUST be recorded with justification, scope, and rollback plan, and
  approved by the responsible maintainer before merge.
- Delivery complexity MUST be proportional to the specified requirement; anticipatory
  generalizations and abstractions without concrete use MUST be rejected.

## Governance

This constitution supersedes any conflicting project practice. When this constitution, a README, the
Makefile, or local conventions conflict, the constitution prevails.

- Amendments MUST be proposed through a reviewable change describing the motivation, the impact on
  existing principles, and the migration plan for existing code and tests.
- Amendments MUST be approved by the responsible maintainer before merge. Amendments that remove or
  incompatibly redefine principles MUST require additional explicit approval.
- Versioning follows SemVer:
  - MAJOR: incompatible removal or redefinition of a principle or governance rule;
  - MINOR: new principle, new section, or material expansion of existing guidance;
  - PATCH: clarification, wording fix, or refinement without semantic change.
- Every review MUST verify compliance with this constitution. Violations MUST be treated as a merge
  blocker, not as technical debt.
- The version history MUST be kept in the amendment's impact report until publication and, after
  the commit, in the Git history.
- Operational guidance for execution and debugging MUST live in the READMEs and the Makefile,
  maintained according to Principle III.

**Version**: 1.1.0 | **Ratified**: 2026-10-21 | **Last Amended**: 2026-10-21
