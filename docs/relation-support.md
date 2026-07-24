# Relation Family Support

Generated from accepted relation-family inventory v1 (`aae2984ba57c42a387081c0f601025f127a32104c2662cb8fa9855b4cfd61bcc`).

| Family | State | Persisted graph relations | Coverage |
| --- | --- | --- | --- |
| `structural_type` | `active` | `legacy:contains`, `legacy:imports`, `legacy:calls` | provider-owned structural facts with parser-strength coverage |
| `package_manifest` | `active` | `legacy:depends-on` | accepted manifest dependencies only |
| `test` | `active` | `extended:tests` | statically resolved calls and imports from recognized test paths |
| `route_protocol` | `active` | `extended:routes-to` | recognized static route registrations only; dynamic registrations abstain |
| `configuration_environment` | `active` | `extended:configures` | recognized configuration files and static environment keys; values are excluded |
| `deployment_infrastructure` | `active` | `extended:deploys` | recognized infrastructure configuration files; resource detail remains partial |
| `static_data_access` | `active` | `extended:reads`, `extended:writes` | recognized static read and write calls only; dynamic paths abstain |
| `inferred_similarity` | `optional_disabled` | — | not advertised while optional quality and resource gates are unmet |
| `inferred_co_change` | `optional_disabled` | — | not advertised while optional quality and freshness gates are unmet |

Active rows are persisted through the normalized SQLite graph and consumed by the existing bounded relation and analysis calls. Optional inferred rows remain unavailable until their independent quality, determinism, freshness, package, memory, and platform gates pass.
