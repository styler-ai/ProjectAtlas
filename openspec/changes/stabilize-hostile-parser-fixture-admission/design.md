## Context

`run_adversarial_process_suite` owns test-only hostile peer policy. Its `Case::no_progress` value currently flows into both `ResidentParserSession::launch_command` and `ResidentParserSession::parse`, so the 500 ms operation-stall budget also limits ordinary Windows process creation and containment admission. The `progress-endless` fixture also replaces the shared case deadline before launch, allowing a launch or admission `DeadlineExceeded` to satisfy a broad deadline expectation without reaching the response phase. The same suite's resident-reuse preflight separately hardcodes a one-second launch/admission allowance.

## Goals / Non-Goals

**Goals:**

- Preserve the 500 ms bound at the phase intentionally stalled by pre-ready, admission, and progress-stall fixtures.
- Let every other hostile fixture perform one bounded launch/admission attempt before its existing exact typed assertion.
- Retain mandatory child/thread cleanup and keep all production parser constants unchanged.

**Non-Goals:**

- Retrying cases, weakening expected errors, changing production timing, or changing the #391 healthy-recovery policy.
- Introducing a reusable policy framework for one test harness.

## Decisions

1. Add one test-only bounded non-stall launch/admission allowance at the existing harness owner. The closed pre-ready and admission stall scenarios retain the case's short no-progress value; all other cases and both resident-reuse launches use the non-stall allowance. This adds no production API or abstraction. A separate case type was rejected because the scenario set is closed inside one function.
2. Continue passing `Case::no_progress` unchanged to `ResidentParserSession::parse`. Start any fixture-specific operation deadline only after successful launch/admission, leaving the ordinary bounded launch deadline intact. This preserves the progress-stall contract while preventing launch scheduling from consuming the response-phase deadline.
3. Keep one `operate` call per hostile case and preserve `error_matches`, cleanup-failure rejection, process-spawn lease checks, and the healthy restart. Require deadline expectations to name their exact phase (`request response` for endless progress and `request write` for the blocked writer); an earlier launch/admission deadline is a mismatch. Retries were rejected because they could mask leaked ownership or a first-attempt cleanup failure.
4. Protect the selector and exact deadline classification with focused deterministic checks and the complete process-owning adversarial suite. The progress-endless peer deliberately delays launch/admission longer than its later operation deadline, proving the two bounds are independent. The selector and comparison are constant-time and allocation-free; the fixture adds one bounded test-only sleep but no extra child processes, threads, filesystem I/O, persistent bytes, or storage work.

## Risks / Trade-offs

- [A newly added launch-stall scenario is not classified as short] -> Keep the closed selector and its focused positive/negative classification coverage beside the harness.
- [A longer launch allowance hides a leaked child] -> Preserve one attempt plus the existing mandatory cleanup, process-spawn lease, and healthy-restart checks after every case.
- [An operation deadline is accidentally reused for launch or a broad match accepts the wrong phase] -> Delay the deadline-owned peer before admission, derive its deadline after launch, and match the exact operation phase.
- [Test policy drifts into production] -> Keep all new names and logic under `cfg(test)` and assert the relevant production admission constants remain unchanged.
