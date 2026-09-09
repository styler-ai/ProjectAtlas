## 1. Contract and release scope

- [x] 1.1 Define metadata isolation, exact base-retarget proof, and the owning release dependency mapping.

## 2. Implementation and proof

- [x] 2.1 Schedule the existing result job while guarding both source steps, and update the owning workflow-contract assertions and guidance.
- [ ] 2.2 Prove metadata edits preserve source-check identity and readiness, base retargets execute exact comparison proof, and incomplete retarget proof cannot be accepted by later metadata; pass required local and hosted gates.

Verification: `cargo test --locked -p projectatlas-cli --test e2e_delivery issueops_and_workflows_use_behavior_focused_quality_gates -- --exact --nocapture`;
`python .github/scripts/affected-ci-proof.py --self-test`;
strict OpenSpec and planned/candidate IssueOps validation; the normal pre-push hook
and its selected hosted matrix; native Actions check/job/plan and protected-PR
readback for title/body edits, base retarget, overlapping metadata, and issue refresh.
