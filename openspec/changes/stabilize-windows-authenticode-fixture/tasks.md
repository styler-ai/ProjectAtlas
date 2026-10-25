## 1. Specification

- [x] 1.1 Specify the hermetic Windows Authenticode fixture boundary, exact inherited-environment inputs, fail-closed compatibility, release ownership, non-goals, and existing architecture-view applicability.

## 2. Implementation

- [x] 2.1 Make the existing Authenticode fixture deterministic across supported Windows parent shells at the smallest test owner while preserving the production module-qualified cmdlet, signer/status/type/digest checks, malicious-shadow rejection, and unchanged installer behavior.
- [x] 2.2 Replace the combined failure with causal invariant diagnostics and focused negative coverage for lookup shadowing, signature shadowing, unsigned input, invalid signer/status/type, and digest mismatch.

## 3. Verification

- [x] 3.1 Prove the focused test under PowerShell and `cmd.exe`, its serialized installer subgroup, and the normal parallel full E2E/workspace gate without retries or global serialization; obtain hosted Windows proof and retain non-Windows compatibility.
- [x] 3.2 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
