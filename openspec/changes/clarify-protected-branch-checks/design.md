## Context

See proposal.md. GitHub excludes workflow_dispatch job checks from protected-branch requirements. The existing workflow already runs eligible pull_request checks for main and dev; no workflow definition needs to change.

## Goals / Non-Goals

Explain the existing check-source restriction and main-based integration route. Do not add a CI mechanism, change protection or publish a product version.

## Decisions

Clarify the manual-run note in docs/workflow.md and link GitHub's official event restriction. Identify the existing pull_request workflow as the eligible route while keeping the current integration policy unchanged. Do not add synchronization instructions that have not been demonstrated against both protected refs.

## Risks / Trade-offs

Manual success could be mistaken for required-gate success: state the event restriction explicitly. New advice could imply a changed branch policy: leave the existing main-based policy in place.

## Migration Plan

Publish the guide with product/plugin versions unchanged. No runtime, storage, workflow or protection migration is needed. Reverting the prose restores the earlier guide without changing user data.

## Architecture Disposition

N/A: documentation only. CI triggers, jobs, status publication, runtime and storage architecture remain unchanged. The delta requirements own the accuracy of public maintainer guidance.

## Dependencies / Cross-Issue Impact

#718 owns this documentation work under #715. It is independent of #708's indexing implementation and does not change its tasks or acceptance criteria.

## Open Questions

None.

## Verification

Compare the guide with current workflow triggers, branch settings, observed manual-run refusal and GitHub's official rules. Use independent document review and repository-selected gates. A wording-only test would mirror this reversible change and is unnecessary.
