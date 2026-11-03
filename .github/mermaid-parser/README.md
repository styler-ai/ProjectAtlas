# Diagram validation dependencies

This directory owns the locked Mermaid parser used by IssueOps and repository
checks. Its grammar and supported Node range remain pinned in `package.json`.

Mermaid's maintained 11.x dependency range currently excludes the KaTeX version
that fixes [GHSA-238p-pmpm-9mq7](https://github.com/KaTeX/KaTeX/security/advisories/GHSA-238p-pmpm-9mq7).
The scoped override selects KaTeX 0.18.2 while retaining Mermaid 11.16.1 and its
existing Node support. Remove the override when the retained upstream range can
select an unaffected version through ordinary resolution; do not suppress the
advisory instead. The validator only parses diagrams; package exposure alone is
not proof of an exploitable ProjectAtlas path.

Run `npm ci --ignore-scripts --no-audit`, `npm test` and
`npm audit --omit=dev --audit-level=low` in this directory. The behavior test uses
the KaTeX package actually resolved by Mermaid: inherited options cannot enable
trusted rendering, while explicitly trusted rendering and ordinary math still
work. It creates strings only, with no browser insertion or network request.
The existing IssueOps self-test owns valid/invalid diagram and parser behavior.
