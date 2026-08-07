## ADDED Requirements

### Requirement: Reports distinguish average and maximum token impact
ProjectAtlas SHALL report explicit `average_tokens_avoided` and `maximum_tokens_avoided` values from the same admitted telemetry, SHALL retain `tokens_avoided` as the primary compatibility field equal to `average_tokens_avoided`, and SHALL keep the existing all-files result as `maximum_tokens_avoided`.

#### Scenario: Mixed accounting report
- **WHEN** a report contains observed compression, modeled directory-walk navigation, and other modeled categories
- **THEN** both totals include observed compression and the other modeled categories unchanged, while only the average total discounts the directory-walk baseline

#### Scenario: Existing structured consumer
- **WHEN** an existing CLI or MCP consumer reads the v0.4.4 token overview
- **THEN** every existing field remains present and `tokens_avoided` identifies the primary average value

### Requirement: Average folder-navigation policy is reproducible
ProjectAtlas SHALL calculate the average modeled `directory_walk` contribution as `floor(deduped_aggregate_without_projectatlas / 2) - aggregate_with_projectatlas`, SHALL calculate the matching maximum contribution as `deduped_aggregate_without_projectatlas - aggregate_with_projectatlas`, and SHALL apply no 50% adjustment to any other denominator kind.

#### Scenario: One directory-walk event
- **WHEN** a modeled directory-walk baseline is 101 tokens and the Atlas payload is 20 tokens
- **THEN** the average modeled component is 30 tokens and the maximum modeled component is 81 tokens

#### Scenario: Non-folder modeled event
- **WHEN** a modeled event uses `selected_candidates` or another non-directory-walk denominator
- **THEN** its signed contribution is identical in the average and maximum totals

#### Scenario: Payload exceeds average baseline
- **WHEN** the Atlas payload exceeds half of a directory-walk baseline
- **THEN** the average component remains a negative signed value and is not clamped

### Requirement: Baseline deduplication precedes the average policy
ProjectAtlas SHALL preserve the existing modeled-baseline deduplication and SHALL apply the 50% directory-walk policy to the single retained baseline while subtracting every admitted Atlas payload unchanged.

#### Scenario: Repeated folder baseline
- **WHEN** one directory-walk baseline is repeated within its dedupe scope
- **THEN** both totals count the retained baseline once, the average total halves it once, and both totals subtract all emitted Atlas payloads

#### Scenario: Several odd retained folder baselines
- **WHEN** several directory-walk baselines remain after deduplication
- **THEN** ProjectAtlas sums the retained folder baselines, rounds down once after applying 50%, and subtracts the complete aggregate Atlas payload

### Requirement: Existing storage and accounting remain compatible
ProjectAtlas SHALL derive the two values from existing raw or aggregate telemetry without changing the SQLite schema, stored events, pruning, transactions, WAL behavior, buckets, trends, read-avoidance counters, search baselines, or tokenizer calculations, and MAY retain one additional reserved directory-walk overflow dimension so the policy discriminator survives normal dimension capacity.

#### Scenario: Existing database report
- **WHEN** v0.4.4 opens a compatible database created by v0.4.3
- **THEN** it derives average and maximum values without schema migration or stored-event mutation and preserves all other report results

#### Scenario: Predecessor overflow lost the denominator
- **WHEN** an existing generic overflow aggregate contains modeled telemetry whose original denominator cannot be recovered
- **THEN** its contribution remains at the maximum and `average_policy.evidence` explicitly identifies the unclassified overflow fallback

#### Scenario: New directory walk reaches dimension capacity
- **WHEN** a new directory-walk event is routed to bounded dimension overflow
- **THEN** one reserved overflow dimension retains the directory-walk discriminator and the durable average matches raw-event accounting

#### Scenario: Raw and durable aggregate parity
- **WHEN** equivalent telemetry is reported directly from raw events and after a real SQLite write/read round trip
- **THEN** average and maximum totals are identical across both paths

### Requirement: TUI emphasizes average and explains maximum
The token overview TUI SHALL use average tokens avoided as its primary hero and equation result, SHALL retain the existing visual composition, and SHALL add one compact lower bar for maximum tokens avoided with copy identifying the 50% folder-scope average and all-files maximum policies.

#### Scenario: Desktop dashboard
- **WHEN** the token overview renders at the supported desktop width
- **THEN** the average hero is dominant, the maximum bar is visible below it, and existing navigation, composition, source, notes, status, and optional Atlas sections remain recognizable

#### Scenario: Narrow dashboard
- **WHEN** the token overview renders at the supported narrow width
- **THEN** both values and their policy distinction remain readable without overlap or truncation of core information

#### Scenario: Theme variants
- **WHEN** the dashboard renders in dark, light, and terminal themes
- **THEN** average and maximum labels, values, bars, signed states, and surrounding panel boundaries remain visually distinguishable without relying on color alone
