# CLI E2E ownership split move map

This map freezes the complete pre-move contract before e2e.rs is deleted. The machine-readable source of truth is v050-cli-e2e-inventory.json.

- Baseline commit: b8f368c0f1e2299b7d0cbb0c3646bb4c238dbceb
- Baseline e2e.rs SHA-256: e26c7b9d450b105e09c2259b243f95a1fddb26cd8b64e176379149ca8050b43c
- Inventoried pre-deletion e2e.rs SHA-256: 942b802ab4c215f1742d2c41f35eb29654946da8e8372218e0d1a787cc3c4757 (the only edits before deletion rewired contract assertions/selectors to their split owners)
- Symbols inventoried: 427 top-level items; tests: 158
- Post-move union proof compares the tests.name set in the JSON with the test names listed by all five e2e_* binaries.

## Durable binary ownership

| Binary | Owns | Tests |
| --- | --- | ---: |
| e2e_lifecycle | lifecycle and database contracts | 37 |
| e2e_delivery | installer, release, packaged, and plugin contracts | 56 |
| e2e_navigation | CLI, MCP, graph, document, and language navigation contracts | 32 |
| e2e_worktrees | worktree, watcher, freshness, and federation contracts | 17 |
| e2e_maintenance | purpose, lint, telemetry, and TUI contracts | 16 |

Only concrete process, repository, JSON, platform, and packaged-contract support referenced by more than one owner is repeated/shared at the integration boundary. No new framework, catch-all support module, product code, crate, dependency, or symmetric empty binary is introduced.

## Enforced inventory contract

The e2e_delivery integration binary loads docs/v050-cli-e2e-inventory.json on every normal test run. Schema 2 records the five split source-file SHA-256 digests after normalizing CRLF/CR to LF and excluding absolute paths and line metadata; these digests cover complete test bodies, assertions, and recorded contract facets. The multiply-owned process, repository, JSON, SQLite, and platform helpers have one private owner at `crates/projectatlas-cli/tests/support/mod.rs`; the executable checker freezes the complete normalized support module with its own fixed SHA-256 digest, so constants, type shapes, and helper bodies cannot drift or be concealed by updating a current binary digest. It also validates the frozen baseline and pre-deletion identities, complete symbol and fixture names/owners/attributes, every recorded facet line, and the immutable pre-move selector inventory before rescanning test names, duplicate/missing ownership, attributes/platform gates, and the accepted e2e_*.rs binary boundary (including rejection of legacy e2e.rs). It then compares every recorded CI/release selector by normalized relative workflow path and selector text, recognizing both Cargo `--test e2e_*` and `--test=e2e_*` forms. Its positive and tamper cases prove source assertion/facet, shared support (including the Git scrub-list), symbol, fixture, baseline, legacy-target, and spaced/equal-form legacy or unknown selector drift fails closed. The two checker tests are recorded outside the 158 moved tests so the frozen pre-move union remains exact.

## Test-to-domain move map

Every pre-move test is listed with source bounds, preserved attributes, and exactly one owner.

| Test | Pre-move lines | Owner | Attributes |
| --- | ---: | --- | --- |
| runtime_info_does_not_create_projectatlas_directory | 348-395 | e2e_lifecycle | #[test] |
| installed_candidate_version_is_consistent_across_cli_runtime_and_token_tui | 398-480 | e2e_lifecycle | #[test] |
| token_tui_cli_respects_selected_terminal_viewport | 483-621 | e2e_maintenance | #[test] |
| derived_snapshot_cli_round_trips_without_replacing_authored_state | 625-769 | e2e_lifecycle | #[cfg(feature = "derived-snapshot")] #[test] |
| detailed_relation_cli_bounds_the_exact_json_envelope | 772-1197 | e2e_navigation | #[test] |
| impact_analysis_deadline_and_mcp_cancellation_release_resources | 1200-1372 | e2e_navigation | #[test] |
| linux_btrfs_subvolume_database_supports_cli_and_persistent_mcp_reopen | 1377-1484 | e2e_lifecycle | #[cfg(target_os = "linux")] #[test] #[ignore = "requires a supplied native Btrfs subvolume and exact candidate runtime"] |
| persistent_mcp_stdin_does_not_block_repository_startup_probes | 1487-1585 | e2e_lifecycle | #[test] |
| installed_candidate_without_git_keeps_navigation_and_typed_vcs_unavailability | 1588-1774 | e2e_navigation | #[test] |
| parser_pack_disable_does_not_require_default_user_storage | 1778-1806 | e2e_lifecycle | #[cfg(feature = "optional-parser-supervisor")] #[test] |
| parser_pack_supported_only_commands_refuse_unsupported_macos_before_state_access | 1810-2116 | e2e_lifecycle | #[cfg(all(target_os = "macos", feature = "optional-parser-supervisor"))] #[test] |
| optional_parser_pack_real_archive_normal_runtime_lifecycle | 2121-2629 | e2e_lifecycle | #[cfg(feature = "optional-parser-supervisor")] #[test] #[ignore = "requires one exact workflow-built optional parser-pack archive"] |
| cli_navigation_output_survives_telemetry_write_failure | 2632-2661 | e2e_navigation | #[test] |
| cli_invocations_with_one_label_use_distinct_sealed_instances | 2664-2700 | e2e_navigation | #[test] |
| settings_reports_content_free_telemetry_without_recording | 2703-3297 | e2e_lifecycle | #[test] |
| settings_rejects_untrusted_publication_with_retained_text | 3300-3392 | e2e_lifecycle | #[test] |
| settings_reports_supported_predecessor_without_migration | 3395-3409 | e2e_lifecycle | #[test] |
| supported_predecessor_recovery_preserves_explicit_database_selection | 3603-3758 | e2e_lifecycle | #[test] |
| init_and_scan_migrate_both_released_schema_layouts | 3761-3777 | e2e_lifecycle | #[test] |
| mcp_clean_shutdown_seals_runtime_instances_across_restarts | 3884-3950 | e2e_lifecycle | #[test] |
| init_bootstrap_creates_db_scan_report_and_host_configs | 3953-4027 | e2e_lifecycle | #[test] |
| holistic_agent_worktree_flow_keeps_local_atlases_isolated_across_cli_watch_and_mcp | 4031-5274 | e2e_worktrees | #[test] #[ignore = "dedicated hosted cross-platform holistic worktree proof"] |
| scan_refuses_unverified_registered_worktree_boundary_before_publication | 5277-5357 | e2e_worktrees | #[test] |
| git_control_roots_return_typed_worktree_guidance_without_state | 5360-5584 | e2e_worktrees | #[test] |
| explicit_config_rebases_implicit_database_from_descendant_and_git_manager | 5587-5634 | e2e_worktrees | #[test] |
| implicit_bare_root_refuses_before_opening_a_future_schema_database | 5637-5747 | e2e_lifecycle | #[test] |
| implicit_bare_root_refusal_is_database_state_agnostic | 5750-5823 | e2e_lifecycle | #[test] |
| implicit_bare_root_commands_preserve_live_wal_and_authored_state | 5826-5923 | e2e_lifecycle | #[test] |
| init_no_scan_preserves_existing_config_and_is_idempotent | 5926-5994 | e2e_lifecycle | #[test] |
| init_reports_host_config_failure_before_nonzero_exit | 5997-6045 | e2e_lifecycle | #[test] |
| init_preserves_flat_config_and_uses_it_for_first_scan | 6048-6109 | e2e_lifecycle | #[test] |
| init_explicit_config_creates_selected_config_and_reports_it | 6112-6174 | e2e_lifecycle | #[test] |
| init_explicit_subdir_config_scans_the_repo_root | 6177-6236 | e2e_lifecycle | #[test] |
| root_set_preserves_flat_config_for_generated_mcp_configs | 6239-6282 | e2e_lifecycle | #[test] |
| plugin_installers_require_matching_runtime_version | 6285-6919 | e2e_delivery | #[test] |
| windows_installer_fresh_path_probe_respects_machine_precedence | 6923-7439 | e2e_delivery | #[cfg(windows)] #[test] |
| packaged_skill_routes_startup_and_registered_worktrees | 7442-7635 | e2e_lifecycle | #[test] |
| repository_guidance_keeps_atlas_state_local_and_legacy_export_optional | 7638-8111 | e2e_maintenance | #[test] |
| repository_delivery_and_dependency_policy_is_enforced | 8129-8718 | e2e_maintenance | #[test] |
| filtered_custom_harness_contract_rejects_timeout_drift | 8756-8761 | e2e_delivery | #[test] |
| filtered_custom_harness_contract_rejects_fractional_timeout | 8764-8769 | e2e_delivery | #[test] |
| filtered_custom_harness_contract_rejects_suffixed_command | 8772-8777 | e2e_delivery | #[test] |
| filtered_custom_harness_contract_rejects_suffixed_step_name | 8780-8785 | e2e_delivery | #[test] |
| filtered_custom_harness_contract_rejects_drift_after_suffixed_step | 8788-8793 | e2e_delivery | #[test] |
| filtered_custom_harness_contract_rejects_timeout_borrowed_from_unnamed_step | 8796-8801 | e2e_delivery | #[test] |
| issueops_and_workflows_use_behavior_focused_quality_gates | 8804-9670 | e2e_delivery | #[test] |
| macos_all_features_warning_gate_contract_is_exact | 9673-9752 | e2e_delivery | #[test] |
| plugin_installer_writes_real_harness_configs | 9755-9845 | e2e_delivery | #[test] |
| posix_installer_accepts_symlinked_runtime_path | 9849-9909 | e2e_delivery | #[test] #[cfg(unix)] |
| windows_installer_recovery_operation_preserves_config_selection | 9913-10041 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_release_binary_installer_uses_versioned_runtime_when_stable_mirror_is_locked | 10045-10991 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_retires_only_exact_child_and_reports_retry_failure | 10995-11610 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_preserves_unready_and_ambiguous_processes | 11614-12355 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_requires_exact_codex_plugin_state | 12359-12572 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_binds_generated_config_digest_to_validated_bytes | 12576-12762 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_rejects_changed_exited_and_inaccessible_processes | 12766-13197 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_classifies_exit_after_final_identity_check | 13201-13325 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_requires_trusted_authenticode_cmdlet | 13329-13441 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_obsolete_mcp_handoff_requires_exact_codex_registry | 13445-13598 | e2e_delivery | #[test] #[cfg(windows)] |
| installer_workflow_pin_reports_preserve_exact_rc_identity | 13674-13787 | e2e_delivery | #[test] |
| plugin_update_replaces_stale_runtime_configs_and_launches_new_mcp | 13790-14279 | e2e_delivery | #[test] |
| plugin_update_skips_non_official_codex_marketplace | 14282-14340 | e2e_delivery | #[test] |
| plugin_update_leaves_current_codex_marketplace_untouched_and_repairs_stale_skill | 14343-14478 | e2e_delivery | #[test] |
| plugin_update_repairs_current_codex_plugin_with_stale_source_manifest | 14481-14587 | e2e_delivery | #[test] |
| windows_plugin_update_preserves_prior_integration_when_all_replacement_adds_fail | 14591-14594 | e2e_delivery | #[test] #[cfg(windows)] |
| posix_plugin_update_preserves_prior_integration_when_all_replacement_adds_fail | 14598-14601 | e2e_delivery | #[test] #[cfg(unix)] |
| posix_plugin_lock_rejects_indirection_and_survives_crash | 14883-15229 | e2e_delivery | #[test] #[cfg(unix)] |
| windows_plugin_update_serializes_restore_before_the_next_installer_reads_state | 15325-15328 | e2e_delivery | #[test] #[cfg(windows)] |
| posix_plugin_update_serializes_restore_before_the_next_installer_reads_state | 15332-15335 | e2e_delivery | #[test] #[cfg(unix)] |
| windows_plugin_update_fails_closed_when_lock_root_cannot_be_canonicalized | 15640-15708 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_plugin_update_refuses_retained_recovery_state_before_mutation | 15712-15715 | e2e_delivery | #[test] #[cfg(windows)] |
| posix_plugin_update_refuses_retained_recovery_state_before_mutation | 15719-15722 | e2e_delivery | #[test] #[cfg(unix)] |
| windows_plugin_update_refuses_unavailable_or_ambiguous_inventory | 15809-15812 | e2e_delivery | #[test] #[cfg(windows)] |
| posix_plugin_update_refuses_unavailable_or_ambiguous_inventory | 15816-15818 | e2e_delivery | #[test] #[cfg(unix)] |
| posix_plugin_inventory_without_jq_rejects_split_object_fields | 15943-16008 | e2e_delivery | #[test] #[cfg(unix)] |
| posix_plugin_restore_rejects_hostile_paths_and_retains_recovery_state | 16012-16035 | e2e_delivery | #[test] #[cfg(unix)] |
| windows_plugin_restore_rejects_config_directory_and_retains_recovery_snapshot | 16397-16472 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_plugin_restore_rejects_cache_junction_and_retains_recovery_snapshot | 16476-16574 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_plugin_snapshot_rejects_reparse_above_codex_home_before_mutation | 16578-16667 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_plugin_snapshot_cleanup_refuses_path_swap_without_outside_deletion | 16671-16758 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_plugin_snapshot_cleanup_failure_retains_usable_direct_snapshot | 16762-16885 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_installer_without_codex_reports_clean_skip | 16889-16973 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_release_binary_installer_repairs_stale_mirror_without_registering_it | 16977-17322 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_release_binary_installer_rejects_checksum_mismatch | 17326-17404 | e2e_delivery | #[test] #[cfg(windows)] |
| windows_release_binary_only_rejects_invalid_runtime_without_fallback | 17408-17501 | e2e_delivery | #[test] #[cfg(windows)] |
| posix_release_binary_installer_rejects_checksum_mismatch | 17505-17573 | e2e_delivery | #[test] #[cfg(unix)] |
| bare_relative_projectatlas_config_path_drives_scan_map_and_lint | 17576-17706 | e2e_navigation | #[test] |
| explicit_database_binding_is_used_by_cli_and_mcp_admin_surfaces | 17709-17864 | e2e_lifecycle | #[test] |
| generated_mcp_config_preserves_explicit_conventional_database_authority | 17867-17907 | e2e_lifecycle | #[test] |
| scan_overview_and_token_flow | 17910-18857 | e2e_navigation | #[test] |
| token_cli_and_mcp_preserve_average_maximum_edge_accounting | 18860-18980 | e2e_maintenance | #[test] |
| root_and_metadata_validation_flow | 18983-19353 | e2e_lifecycle | #[test] |
| mcp_server_stays_bound_to_one_project_database | 19356-19460 | e2e_lifecycle | #[test] |
| no_telemetry_readonly_cli_smoke | 19463-19617 | e2e_lifecycle | #[test] |
| mcp_tools_list_preserves_frozen_contracts_without_index_state | 19798-19823 | e2e_navigation | #[test] |
| agent_efficiency_cli_mcp_contract_is_typed_read_only_and_isolated | 19826-20057 | e2e_navigation | #[test] |
| large_repository_agent_funnel_stays_bounded | 20060-20225 | e2e_navigation | #[test] |
| symbols_watch_and_legacy_cleanup_flow | 20228-20406 | e2e_worktrees | #[test] |
| real_scan_resolves_import_alias_called_by_across_core_languages | 20409-20628 | e2e_navigation | #[test] |
| packaged_cli_surface_preserves_frozen_routes_and_defaults | 20631-20768 | e2e_delivery | #[test] |
| packaged_cli_commands_own_their_real_sqlite_effects | 20771-21424 | e2e_delivery | #[test] |
| mcp_advertised_tools_own_their_real_sqlite_effects | 21427-22178 | e2e_delivery | #[test] |
| mcp_stdio_serves_toon_tool_payloads | 22181-22399 | e2e_delivery | #[test] |
| ranked_files_and_next_include_bounded_reasons | 22402-22534 | e2e_navigation | #[test] |
| cli_navigation_rows_propagate_nonempty_typed_graph_evidence | 22537-22791 | e2e_navigation | #[test] |
| cli_federation_is_explicit_read_only_and_fails_closed_on_a_stale_late_root | 22794-22903 | e2e_worktrees | #[test] |
| mcp_federation_uses_the_existing_relation_tool_without_telemetry_writes | 22906-22967 | e2e_worktrees | #[test] |
| indexed_reads_use_scanned_project_root_from_any_cwd | 23169-23292 | e2e_worktrees | #[test] |
| scan_honors_configured_excludes_and_cli_fuzzy_search | 23295-23421 | e2e_lifecycle | #[test] |
| ignore_commands_preserve_manual_layer_while_gitignore_updates_apply | 23424-23746 | e2e_lifecycle | #[test] |
| default_scan_drops_stale_nodes_after_prefix_exclude_config_change | 23749-23823 | e2e_lifecycle | #[test] |
| vue_composition_api_summary_uses_bindings | 23826-23874 | e2e_lifecycle | #[test] |
| javascript_summary_ignores_locals_and_object_stub_methods | 23877-23979 | e2e_lifecycle | #[test] |
| structural_summaries_cover_declarative_files_and_projectatlas_inputs | 23982-24173 | e2e_lifecycle | #[test] |
| classified_document_navigation_agrees_across_cli_and_mcp | 24176-24791 | e2e_navigation | #[test] |
| default_scan_indexes_complete_accepted_core_surface | 24794-24942 | e2e_navigation | #[test] |
| language_fixture_summaries_match_baselines | 24945-24998 | e2e_navigation | #[test] |
| map_and_lint_honor_configured_exclude_path_prefixes | 25001-25059 | e2e_navigation | #[test] |
| first_default_scan_skips_stale_legacy_map_purposes | 25062-25115 | e2e_navigation | #[test] |
| scan_does_not_overwrite_agent_purpose_with_legacy_header | 25118-25179 | e2e_navigation | #[test] |
| mcp_config_discovers_flat_config_from_db_root | 25182-25259 | e2e_navigation | #[test] |
| files_command_normalizes_windows_style_folder_filters | 25262-25289 | e2e_navigation | #[test] |
| scan_does_not_exclude_repository_under_excluded_parent_name | 25292-25325 | e2e_navigation | #[test] |
| notify_watch_refreshes_symbols_after_file_change | 25328-25444 | e2e_worktrees | #[test] |
| normal_reads_do_not_serve_offline_stale_index_state | 25447-25460 | e2e_worktrees | #[test] |
| compiler_config_utf8_bom_refreshes_through_cli_and_mcp | 25463-25567 | e2e_navigation | #[test] |
| configured_module_aliases_resolve_across_adapters_and_refresh_atomically | 25570-25844 | e2e_worktrees | #[test] |
| dependency_aware_refresh_re_resolves_unchanged_inbound_callers | 25922-25968 | e2e_worktrees | #[test] |
| csharp_symbol_identity_boundary_preserves_full_and_incremental_publication | 26084-26294 | e2e_navigation | #[test] |
| deep_qualified_symbol_parents_preserve_full_and_incremental_publication | 26297-26412 | e2e_navigation | #[test] |
| partial_markdown_limit_persists_without_losing_complete_publication | 26415-26498 | e2e_navigation | #[test] |
| incremental_refreshes_converge_with_clean_scan_results | 26501-26708 | e2e_worktrees | #[test] |
| watch_once_preserves_unchanged_deep_summary_and_text_index | 27918-28074 | e2e_worktrees | #[test] |
| watch_once_skips_unchanged_empty_native_parse | 28077-28120 | e2e_worktrees | #[test] |
| watch_once_preserves_manifest_symbol_summary | 28123-28170 | e2e_worktrees | #[test] |
| watch_once_detects_new_files_folders_text_and_symbols | 28173-28235 | e2e_worktrees | #[test] |
| full_repository_intelligence_flow_indexes_database_and_commands | 28238-28465 | e2e_navigation | #[test] |
| gradle_dsl_tasks_are_symbols_and_file_ranking_signals | 28468-28607 | e2e_navigation | #[test] |
| parity_alias_passes_clean_repository | 28610-28673 | e2e_navigation | #[test] |
| agent_purpose_and_health_resolution_gate_flow | 28676-28806 | e2e_maintenance | #[test] |
| generated_file_purpose_suggestions_require_agent_approval | 28809-29096 | e2e_maintenance | #[test] |
| purpose_review_batch_applies_agent_review_without_raw_sql | 29099-29261 | e2e_maintenance | #[test] |
| purpose_review_adapters_enforce_shared_input_budgets | 29264-29481 | e2e_maintenance | #[test] |
| conditional_purpose_review_cli_reconciles_source_before_apply | 29484-29850 | e2e_maintenance | #[test] |
| persistent_mcp_purpose_review_reconciles_source_before_apply | 29853-30064 | e2e_maintenance | #[test] |
| powershell_summary_preserves_hyphenated_function_names | 30067-30119 | e2e_navigation | #[test] |
| generated_purpose_queue_avoids_generic_and_asset_noise | 30122-30324 | e2e_maintenance | #[test] |
| lint_formats_share_typed_cli_and_mcp_report | 30327-30643 | e2e_maintenance | #[test] |
| lint_purpose_levels_require_agent_review_at_configured_scope | 30646-30898 | e2e_maintenance | #[test] |
| search_and_symbol_slice_are_bounded_and_identity_safe | 30901-31184 | e2e_navigation | #[test] |
| skipped_and_failed_symbol_builds_keep_a_consistent_projection | 31187-31285 | e2e_navigation | #[test] |
| mcp_test_shutdown_runs_after_primary_failure_without_hiding_it | 32985-33005 | e2e_lifecycle | #[test] |
| codex_schema_audit_rejects_every_definition_and_reference_form | 33984-33994 | e2e_delivery | #[test] |
| packaged_contract_accepts_only_owned_state_dependent_updates | 34281-34379 | e2e_delivery | #[test] |
| windows_fixture_cleanup_requires_exact_process_identity | 35953-35997 | e2e_delivery | #[test] #[cfg(windows)] |
| health_check_reports_duplicate_temp_folders | 36851-36875 | e2e_maintenance | #[test] |
| purpose_file_seed_command_surface_is_removed | 36878-36890 | e2e_maintenance | #[test] |
| init_map_and_lint_flow_uses_rust_implementation | 36893-36987 | e2e_maintenance | #[test] |

## Helper, fixture, and contract-facet inventory

The JSON symbols array contains every top-level helper, constant, enum, and struct with source bounds, attributes, and owning binary set. Its fixtures array contains every top-level fixture constant. `contract_facets` records every source line that mutates environment state, sets a timeout or deadline, owns process cleanup, constructs isolated processes or fixtures, or exercises a packaged/install/release route. The normal inventory contract checks execute these claims: symbol and fixture records must retain their frozen identities and current ownership, each facet line must still exist in the split sources, and the complete normalized support module must match its accepted fixed digest.

## Selector migration

The JSON keeps exact baseline selectors and post-move selector lines for CI, release, and optional-parser workflows. Selector semantics, timeout blocks, shell/platform gates, ignored selection, exact selection, output capture, and process serialization flags remain attached to their original commands.

## Existing separate suites

- installer_trust_boundaries.rs
- language_runtime_compatibility.rs
- lint_diagnostics.rs
- optional_parser_worker_failure.rs
- optional_parser_worker_platform.rs
- parser_launch_authority.rs
- parser_supervisor_adversarial.rs

## Architecture relationship

The existing CLI E2E contract-ownership split diagram remains semantically true: one shared support boundary feeds the five durable owners, and CI/release selectors point to the corresponding owner binary. No diagram change is required.

## Post-move contract proof

The exact union check reads the 158 frozen test names from the JSON inventory and the declarations in all five split files. It reports expected `158`, actual `158`, no duplicates, no missing names, and no unexpected names. The resulting source ownership counts are lifecycle `37`, delivery `56`, navigation `32`, worktrees `17`, and maintenance `16`.

Each split binary was compiled and run independently with all features. Windows execution ran lifecycle `34 passed, 1 ignored`, delivery `47 passed`, navigation `32 passed`, worktrees `16 passed, 1 ignored`, and maintenance `16 passed`; the ignored cases retain their supplied-archive and hosted-holistic requirements. Release-mode compilation emitted all five binaries, and each binary's `--list` route completed successfully. The exact legacy `--test e2e` and `e2e.rs` selectors are absent from repository workflows and scripts.
