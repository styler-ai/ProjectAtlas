# Active context

## Sole active owner and release authority

Only #741 / PR #743 is active. Normally merge it and verify BOTH CLOSED before
resuming another issue. #731 / PR737 are CLOSED via main a5b1bebcaa79c8386b68e9c247eff73aa11e5e15.
Public head before this candidate is ab3a9fa1acd8629b77b98b7c2b70864ba96a865f.
Selected Git HEAD/status and the native-push receipt own the candidate identity.
Local/live tasks remain1/5 (only1.1); all five acceptance rows OPEN. PR ready/notdraft,
base main, milestone37, body Refs741. Restore sole Fixes741 only at final readiness.

Active goal: publish and independently verify v0.5.1-rc1, non-draft prerelease,
Latest=false; stable050 stays Latest. Stable publication/milestone closure require
later instruction. No RC exists. Keep parked739/738/708/715 checkouts/privateDBs.
Queued Codex CI adoption follows741 normalmerge+BOTHCLOSED; inspect existing gaps
before creating one focused issue: /tmp/projectatlas-codex-ci-followup.json.
Dev retired and720CLOSED/not_planned, receipt /tmp/projectatlas-retire-dev.json.

## Current native-policy candidate

Primary implemented; existing Luna Max review_741 independently found no remaining
source/fixture blocker and approved publication for native proof, NOT acceptance.
Current owning files project_state.rs, project_state_linux.rs, project_state_macos.rs,
project_state_tests.rs, CI, direct already-locked rustix edge, docs and OpenSpec.
No schema/DER/SEM change: accepted main DER24/SEM23; final715 DER must exceed every
private contract. Cargo.lock adds only the CLI direct rustix dependency edge.

Stage names are bound to held file and parent-directory identities before metadata,
bytes, persist and cleanup. Foreign replacement disables NamedTempFile cleanup via
RetainedStaging. Windows query-only FILE_READ_ATTRIBUTES keeps DELETE-only data
sharing and has native read/write exclusion control; Unix identity uses lstat+FD
metadata, preserving write-only destinations. Failed empty-probe unlink retains its
file authority for ownership diagnosis, without retrying an uncertain unlink.

Linux captures admitted inode, uid/gid/mode and six native access attributes through
NOFOLLOW FD: POSIX/NFSv4 ACL, SELinux and Smack object/exec/mmap labels. Named empty
probes are reused only on complete policy equality and need no setters. Otherwise
only the owned empty probe is discarded; O_TMPFILE receives bytes/sync while unnamed
under creator policy, then captured metadata/readback/sync before a name is linked.
This avoids both Smack pre-open handles and SELinux write-after-SID-change denial.
Unsupported preservation refuses causally, keeping valid output. Source inode/policy
and absent Replace targets are rechecked immediately before publication.

Mac isolates existing-target payloads if parent FILE_INHERIT ACLs or a different
owner could broaden access. The workspace remains WRITER-owned, ACL-empty,0700 and
identity-bound before any payload. Fresh outputs retain direct-parent inheritance.
Cleanup is checked nonrecursive remove_dir; foreign/nonempty workspaces survive;
postcommit cleanup errors explicitly report committed publication.
Mac/Windows ReplacementSource retains a NOFOLLOW source FD and native ACL or
OWNER|GROUP|DACL snapshot, verifies before apply and again before Replace. Windows
copies from held source, retaining NULL/empty/P/AR/ordered-ACE semantics and only
the previously proved regular-file AI normalization.

## Current tests and remaining native proof

PASS current26Linux owning units+1explicit privileged ignored, actual CLI/MCP writer
4.93s, all-target/all-feature CLI Clippy7.07s, strictstrings, fmt, OpenSpec and scoped
IssueOps1/5. Logs /tmp/projectatlas-741-native-policy-final-{unit,clippy}.log,
-destination-snapshot-writer-e2e.log, -native-policy-{strings,issueops}.log.
Causal destination baseline removing only final checks FAILS with overwritten
foreign destination; restored checks pass replace/remove/mode/appear. Log
/tmp/projectatlas-741-destination-policy-causal-before.log. Write-only0200 positive
passes. Earlier stage-leaf causal failure/publish evidence remains
/tmp/projectatlas-741-stage-entry-causal-before.log (not the compile-typo -before log).

Native tests are implemented and EXPLICITLY selected by CI, but not yet run here:
- Linux root test builds as runner; sudo runs only exact ignored native test binary.
  All replacement registry roles get real security.* labels and uid/gid65534;
  verify exact labels/mode/ownership, bytes, outsidehardlinks and failure cleanup.
  setpriv drops SYS_ADMIN/MAC_ADMIN: equal-policy helper skips setters; actual
  mismatched publisher must refuse EPERM without replacing source/leaking stages.
  This is kernel xattr STORAGE proof, not an enforced SELinux/Smack exploit proof.
- Mac nobody baseline opens ordinary0600 inherited-ACL temp, survives ACL narrowing
  and writes through held FD. Actual stage must deny fresh read/write before copy;
  fresh new-file inheritance stays positive. Foreign private entries and directory
  substitution are retained. A separate UID-mismatch fixture clears parent ACLs
  to isolate the ownership trigger, then proves protected stage/failure cleanup.
- Windows causal replace/remove/DACL/readonly/appear callbacks require refusal,
  unchanged current destination/native policy or retained source, and owned cleanup.
Mac ordinary and all APFS/APFSX/HFS+/HFSX controlled-volume jobs select native ACL
fixture. Disposable outer fixture is traversable so unrelated ancestors cannot
mask nobody controls. Nested attach/detach alone uses sudo; root returned to runner.
Both extracted changed CI bash blocks pass YAML parse and bash-n.

Remaining: normal clean native push; source-bound fresh CLI/MCP/registered/upgrade
proof; actual current four-platform/native security/ACL/four-format results; final
CodeQL/threads, tasks/acceptance and normalmerge+BOTHCLOSED. No acceptance waiver.
Final lifecycle source digest unchanged ee25e9df237172f3c2c924669ac524fe3ffab7ec84038606ec91e57f5ceb761a.
If lifecycle changes, update BOTH docs inventory and support source-contract pins.

## Published ab3a proof remains historical

Normalpush14997 PASS767.64s,34suites/1722pass/10conditionalignored; receipt/log
/tmp/projectatlas-741-acl-normalization-native-push.*. No live14997 lease.
Immutable default binary5cee418c73d4feb46da81d6e8bd3fd1714ddfddb0731fde48504b77e5caf2ae9,
marker /tmp/projectatlas-741-acl-normalization-debug-path.txt, binding133inputs.
Native88514 PASS CLI/MCP3.67s,writer5.18s,registered9.54s and5stable050upgrade layouts
6.08s; identity/ALLapprovedpurposes/config retained and bound bytes unchanged.
These DO NOT cover this native-policy candidate. Use a NEW prefix/source binding.
SourceCI41202161856 TERMINAL: Rust/Linux/Windows/repo/planSUCCESS; bothMacfail only
nested controlled-volume attach PermissionDenied before volume tests; ordinaryMac
23owning+CLI/MCP passed. No APFS/HFS controlled proof yet. Do not rerun old source.

## Latest Advanced Security readback

Fresh PR head ab3a has8open CodeQL alerts:372–376/380 fixture-mediated path flows,
377/379 event-bound checkout/cache-scope findings, all staticallynotactionable;
378 scanner-markedfixed. All7AdvancedSecurity discussion threads already resolved,
no suppression/dismissal. Report /tmp/projectatlas-741-security-comments-20261031-triage.json
rendered; live comments/instances/analyses receipts have same prefix. No new runtime
security tests were run in that read-only triage. Final-source analysis still required.

Creation-mode P2 and CodeQL380 resolved with exact technical replies/readback.
StillOPEN Codex threads: Macfilename PRRT_kwDOQyFgJ86yPTKc, ACL PRRT_kwDOQyFgJ86yP6jJ,
SELinux PRRT_kwDOQyFgJ86yQ2zn. Resolve only after published/native corrected proof.
Linux NFSv4 native filesystem ACL proof and enforced-MAC policy proof remain absent;
do not imply raw label storage proves either. Current host uid1000 has no
passwordless sudo or live SELinux/Smack, so scoped hosted fixtures supply root proof.

## Operating continuity

Read complete installed stable050 Atlas skill/short guide, selected README/context,
integration entry, custom Rust/applicable subskills and accepted OpenSpec after
compaction. Exact741 Atlas brief returns known stable050 policy_drift/full changed0,
tracked739. Preserve privateDB; no orientation reset/installer repair/full scan.
Primary codes; existing LunaMax review_741 is read-only. All shell login:false.
Source /tmp/projectatlas-rc1-test-env.sh before Cargo; shared integration target.
Normal push unsets PROJECTATLAS_NO_TELEMETRY and MCP_CONTRACT_EXECUTABLE override,
uses enabled .githooks and CLEAN candidate. No force/admin merge/hook bypass.
While clean native push or immutable proof runs, no selected edits or competing
build/index/benchmark; poll SAME actual session/process until terminal.

Parked7396eb0dfd+dirt,7388814d6d+dirt,708publicc65dfed/local587ecbe/PR728 and715owned
integration specs remain untouched. 708warm120sample1.127FAIL/focused1.031526partial
retained, noSLOwaiver. Root715children/blockers708/718/721/722/731/732/738/739/741;
closed718/721/722/731/732. Continue serial owners then715 combined installed/plugin/
platform/security/performance/upgrades/packages/publication and independent verify.
