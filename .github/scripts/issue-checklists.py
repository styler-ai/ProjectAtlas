"""Verify GitHub issue checklists mirror OpenSpec tasks."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from urllib.parse import unquote, urlsplit


UNORDERED_LIST_MARKER_RE = r"[-*+]"
TASK_RE = re.compile(
    rf"(?m)^[ ]{{0,3}}{UNORDERED_LIST_MARKER_RE}\s+\[([ xX])\]\s+(.+?)\s*$"
)
TASK_ID_RE = re.compile(r"^(\d+(?:\.\d+)*)\s+")
HEADING_RE = re.compile(r"(?m)^(#{1,6})\s+(.+?)\s*$")
TASK_SECTION_HEADING_RE = re.compile(r"^(\d+(?:\.\d+)*)\.\s+")
HTML_COMMENT_RE = re.compile(r"(?s)<!--.*?(?:-->|$)")
FENCE_RE = re.compile(r"^[ ]{0,3}(`{3,}|~{3,})")
ARCHITECTURE_NA_RE = re.compile(r"(?is)^N/A:\s*(\S(?:.*\S)?)$")
GITHUB_RENDERED_HEADING_PREFIX = "user-content-"
IMPLEMENTATION_TASK_HEADING = "implementation tasks"
ACCEPTANCE_TASK_HEADING = "acceptance and review tasks"
ACCEPTANCE_REVIEW_TASKS = (
    "Intent and outcome review: Confirm the delivered behavior solves the complete issue `Why` and `What Changes`, provides the declared capabilities and release scope, and respects the non-goals at the real user or agent boundary.",
    "Implementation review: Review the complete implementation for correctness, architecture and ownership, applicable Rust and database pattern fit, security, resource bounds, compatibility, and unnecessary complexity; resolve every material finding.",
    "Specification and architecture review: Reconcile the issue, OpenSpec requirements and tasks, source, documentation, and every required architecture diagram; add missing specifications or diagrams or record a reasoned N/A when no view is needed.",
    "Test and proof review: Confirm the owning unit, integration, E2E, fault, concurrency, performance, and platform tests required by the issue are sound, causally exercise real behavior, and cover positive, negative, failure, and compatibility outcomes.",
    "Final readiness review: Confirm every implementation task is complete, all human and automated review feedback is resolved or dispositioned, required local and hosted gates pass, and no behavior or proof boundary remains partial.",
)
COMPLEXITY_LABELS = frozenset(
    {"complexity:low", "complexity:medium", "complexity:high", "complexity:very-high"}
)
MARKDOWN_LINK_RE = re.compile(
    r"\[(?:[^\[\]\n]|\[[^\[\]\n]*\])+\]"
    r"\(\s*<?([^)>\s]+)>?"
    r"(?:\s+(?:\"[^\"\n]*\"|'[^'\n]*'|\([^\)\n]*\)))?\s*\)"
)
MITIGATION_RE = re.compile(
    rf"(?mi)^[ ]{{0,3}}{UNORDERED_LIST_MARKER_RE}\s+\[([ xX])\]\s+(.+?)\s+"
    r"\((OpenSpec|Implementation) tasks:\s*(\d+(?:\.\d+)*(?:\s*,\s*\d+(?:\.\d+)*)*)\)\s*$"
)
EXACT_HEAD_PROOF_RE = re.compile(r"(?i)\bexact[- ]head\b")
EXACT_HEAD_REQUIREMENT_RE = re.compile(
    r"(?i)(?:"
    r"\b(?:must|shall|mandatory|obligat\w*|need(?:s|ed|ing)?|requir\w*|enforc\w*|bind\w*|"
    r"verif\w*|accept\w*|allow\w*|permit\w*)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\bexact[- ]head\b[^.\n!?]{0,120}"
    r"\b(?:must|shall|mandatory|obligat\w*|need(?:s|ed|ing)?|requir\w*|enforc\w*|bind\w*|"
    r"verif\w*|accept\w*|allow\w*|permit\w*)\b"
    r")"
)
EXACT_HEAD_NEGATION_RE = re.compile(
    r"(?i)(?:"
    r"\b(?:is|are)\s+not\s+(?:required|needed|used|accepted|mandatory|allowed|permitted)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\bno\s+exact[- ]head\b"
    r"|"
    r"\bneed\s+not\b[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\bexact[- ]head\b[^.\n!?]{0,120}\bneed\s+not\b"
    r"|"
    r"\b(?:rather than|instead of|independent(?:ly)? of)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\b(?:must not|shall not|should not|cannot|can't)\s+be\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\b(?:do not|don't|does not|doesn't|must not|shall not|should not|cannot|can't|no longer)"
    r"\s+(?:need|require|use|bind|demand|enforce|accept|allow|permit)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\b(?:remove|removing|removed|reject|rejecting|rejected|avoid|avoiding|"
    r"drop|dropping|dropped|retire|retiring|retired|without)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\bexact[- ]head\b[^.\n!?]{0,120}"
    r"\b(?:is|are)?\s*(?:not|no longer)\s+"
    r"(?:required|used|needed|accepted|mandatory|allowed|permitted)\b"
    r")"
)
EXACT_HEAD_SHARED_NEGATION_RE = re.compile(
    r"(?i)\b(?:do not|don't|does not|doesn't|must not|shall not|should not|cannot|can't|no longer)"
    r"\s+(?:need|require|use|bind|demand|enforce|accept|allow|permit)\b"
)
EXACT_HEAD_BARE_ACTION_RE = re.compile(
    r"(?i)^\s*(?:need|require|use|bind|demand|enforce|accept|allow|permit)\b"
)
EXACT_HEAD_BARE_ACTION_ONLY_RE = re.compile(
    r"(?i)^\s*(?:need|require|use|bind|demand|enforce|accept|allow|permit)\s*$"
)
REQUIRED_OPEN_ISSUE_HEADINGS = (
    "why",
    "what changes",
    "capabilities",
    "architecture diagrams",
    "release scope",
    "non-goals",
    "pre-mortem",
)
RELEASE_MILESTONE_RE = re.compile(
    r"^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-00$"
)
REQUIRED_PROPOSAL_HEADINGS = {"why", "what changes", "capabilities", "impact"}
REQUIRED_DESIGN_HEADINGS = {
    "goals / non-goals",
    "decisions",
    "risks / trade-offs",
    "migration plan",
    "dependencies / cross-issue impact",
    "open questions",
}


@dataclass(frozen=True)
class Owner:
    """One issue's inclusive task range."""

    issue: int
    first_task: str | None = None
    last_task: str | None = None


def run(args: list[str]) -> str:
    """Run one fixed command without a shell."""

    process = subprocess.run(
        args,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=120,
        check=False,
    )
    if process.returncode:
        raise SystemExit(
            f"command failed: {json.dumps(args)}\n{process.stderr.strip()}"
        )
    return process.stdout


def gh_json(args: list[str]) -> object:
    return json.loads(run(["gh", *args]))


def gh_api_json(args: list[str]) -> object:
    return json.loads(run(["gh", "api", *args]))


def clean(text: str) -> str:
    return " ".join((text or "").replace("\r", "").split())


def visible_markdown(text: str) -> str:
    """Remove Markdown regions that cannot render authoritative tasks."""

    without_comments = HTML_COMMENT_RE.sub("", text or "")
    visible: list[str] = []
    fence_character = ""
    fence_length = 0
    for line in without_comments.splitlines(keepends=True):
        fence = FENCE_RE.match(line)
        if fence_character:
            marker = fence.group(1) if fence else ""
            if (
                marker
                and marker[0] == fence_character
                and len(marker) >= fence_length
                and not line[fence.end() :].strip()
            ):
                fence_character = ""
                fence_length = 0
            continue
        if fence:
            marker = fence.group(1)
            fence_character = marker[0]
            fence_length = len(marker)
            continue
        if line.startswith("    ") or line.startswith("\t"):
            continue
        visible.append(line)
    return "".join(visible)


def requires_exact_head_proof(text: str) -> bool:
    """Return whether visible prose affirmatively requires commit-head proof."""

    without_link_targets = MARKDOWN_LINK_RE.sub(
        lambda match: match.group(0).replace(match.group(1), ""), text
    )
    for sentence in re.split(r"[.\n!?]+", without_link_targets):
        for clause in re.split(
            r"\s*(?:[:;—]|\b(?:but|however|yet)\b)\s*",
            sentence,
            flags=re.IGNORECASE,
        ):
            assertion_parts = re.split(
                r"\s+\b(?:and|or)\b\s+",
                clause,
                flags=re.IGNORECASE,
            )
            shared_negation = False
            for assertion in assertion_parts:
                inherits_negation = shared_negation and bool(
                    EXACT_HEAD_BARE_ACTION_RE.match(assertion)
                )
                if (
                    EXACT_HEAD_PROOF_RE.search(assertion)
                    and not inherits_negation
                    and not EXACT_HEAD_NEGATION_RE.search(assertion)
                    and EXACT_HEAD_REQUIREMENT_RE.search(assertion)
                ):
                    return True
                shared_negation = bool(
                    EXACT_HEAD_SHARED_NEGATION_RE.search(assertion)
                ) or (
                    shared_negation
                    and bool(EXACT_HEAD_BARE_ACTION_ONLY_RE.match(assertion))
                )
    return False


def github_heading_slug(heading: str) -> str:
    """Return the GitHub-style fragment for one plain Markdown heading."""

    heading = re.sub(r"\s+#+\s*$", "", clean(heading))
    return "".join(
        character
        for character in heading.casefold()
        if character.isalnum() or character in {" ", "-", "_"}
    ).replace(" ", "-")


def markdown_headings(text: str) -> list[tuple[str, int, int, int]]:
    """Return visible headings as fragment, level, start, and end offsets."""

    masked = HTML_COMMENT_RE.sub(
        lambda match: "".join(
            character if character in "\r\n" else " " for character in match.group(0)
        ),
        text or "",
    )
    headings: list[tuple[str, int, int, int]] = []
    next_suffix: dict[str, int] = {}
    used: set[str] = set()
    fence_character = ""
    fence_length = 0
    offset = 0
    for line in masked.splitlines(keepends=True):
        fence = FENCE_RE.match(line)
        if fence_character:
            marker = fence.group(1) if fence else ""
            if (
                marker
                and marker[0] == fence_character
                and len(marker) >= fence_length
                and not line[fence.end() :].strip()
            ):
                fence_character = ""
                fence_length = 0
        elif fence:
            marker = fence.group(1)
            fence_character = marker[0]
            fence_length = len(marker)
        else:
            heading = HEADING_RE.match(line)
            if heading:
                base = github_heading_slug(heading.group(2))
                if base:
                    suffix = next_suffix.get(base, 0)
                    fragment = base if suffix == 0 else f"{base}-{suffix}"
                    while fragment in used:
                        suffix += 1
                        fragment = f"{base}-{suffix}"
                    used.add(fragment)
                    next_suffix[base] = suffix + 1
                    headings.append(
                        (
                            fragment,
                            len(heading.group(1)),
                            offset + heading.start(),
                            offset + heading.end(),
                        )
                    )
        offset += len(line)
    return headings


def markdown_heading_fragments(text: str) -> set[str]:
    """Return rendered heading fragments, including GitHub duplicate suffixes."""

    return {fragment for fragment, _, _, _ in markdown_headings(text)}


def markdown_heading_section(text: str, fragment: str) -> str | None:
    """Return the raw Markdown body owned by one rendered heading fragment."""

    headings = markdown_headings(text)
    for index, (candidate, level, _, end) in enumerate(headings):
        if candidate != fragment:
            continue
        section_end = len(text)
        for _, later_level, later_start, _ in headings[index + 1 :]:
            if later_level <= level:
                section_end = later_start
                break
        return text[end:section_end]
    return None


def mermaid_diagram_blocks(section: str) -> list[str]:
    """Return structurally eligible fenced Mermaid diagram bodies."""

    diagrams: list[str] = []
    fence_character = ""
    fence_length = 0
    mermaid = False
    body: list[str] = []
    for line in section.splitlines():
        fence = FENCE_RE.match(line)
        if fence_character:
            marker = fence.group(1) if fence else ""
            if (
                marker
                and marker[0] == fence_character
                and len(marker) >= fence_length
                and not line[fence.end() :].strip()
            ):
                if mermaid:
                    meaningful = [
                        value
                        for value in (candidate.strip() for candidate in body)
                        if value and not value.startswith("%%")
                    ]
                    if meaningful[:1] == ["---"]:
                        try:
                            frontmatter_end = meaningful.index("---", 1)
                        except ValueError:
                            meaningful = []
                        else:
                            meaningful = meaningful[frontmatter_end + 1 :]
                    if len(meaningful) > 1:
                        diagrams.append("\n".join(body))
                fence_character = ""
                fence_length = 0
                mermaid = False
                body = []
            elif mermaid:
                body.append(line)
        elif fence:
            marker = fence.group(1)
            fence_character = marker[0]
            fence_length = len(marker)
            mermaid = line[fence.end() :].strip().casefold() == "mermaid"
    return diagrams


@lru_cache(maxsize=64)
def mermaid_syntax_is_valid(diagram: str) -> bool:
    """Validate one diagram with the repository-locked Mermaid parser."""

    node = shutil.which("node")
    validator = Path(__file__).resolve().parents[1] / "mermaid-parser" / "validate.mjs"
    mermaid_package = validator.parent / "node_modules" / "mermaid" / "package.json"
    if node is None or not mermaid_package.is_file():
        raise RuntimeError(
            "IssueOps Mermaid validation requires `npm ci --ignore-scripts --prefix "
            ".github/mermaid-parser`"
        )
    try:
        result = subprocess.run(
            [node, str(validator)],
            input=f"{diagram}\n",
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        return False
    return result.returncode == 0


def contains_mermaid_diagram(section: str) -> bool:
    """Return whether a fenced Mermaid block is structurally and syntactically real."""

    return any(mermaid_syntax_is_valid(diagram) for diagram in mermaid_diagram_blocks(section))


def parse_tasks(text: str) -> list[tuple[bool, str]]:
    return [
        (match.group(1).lower() == "x", clean(match.group(2)))
        for match in TASK_RE.finditer(visible_markdown(text))
    ]


def task_id(task: tuple[bool, str]) -> str:
    match = TASK_ID_RE.match(task[1])
    if not match:
        raise SystemExit(f"OpenSpec task has no numeric identifier: {task[1]!r}")
    return match.group(1)


def heading_matches_openspec_tasks(heading: str) -> bool:
    return clean(heading).lower() in {"openspec tasks", "openspec task checklist"}


def heading_matches_implementation_tasks(heading: str) -> bool:
    return clean(heading).casefold() == IMPLEMENTATION_TASK_HEADING


def heading_matches_acceptance_tasks(heading: str) -> bool:
    return clean(heading).casefold() == ACCEPTANCE_TASK_HEADING


def heading_is_task_subsection(heading: str) -> bool:
    return TASK_SECTION_HEADING_RE.match(clean(heading)) is not None


def heading_owns_matching_tasks(
    text: str, headings: list[re.Match[str]], index: int
) -> bool:
    """Return whether one same-level numbered heading owns matching task IDs."""

    heading = headings[index]
    section_match = TASK_SECTION_HEADING_RE.match(clean(heading.group(2)))
    if section_match is None:
        return False
    section_id = section_match.group(1)
    end = len(text)
    for later in headings[index + 1 :]:
        if len(later.group(1)) <= len(heading.group(1)):
            end = later.start()
            break
    section_tasks = parse_tasks(text[heading.end() : end])
    for task in section_tasks:
        identifier = TASK_ID_RE.match(task[1])
        if identifier is not None and (
            identifier.group(1) == section_id
            or identifier.group(1).startswith(f"{section_id}.")
        ):
            return True
    return False


def parse_section_tasks(
    text: str, heading_predicate
) -> list[tuple[bool, str]]:
    tasks: list[tuple[bool, str]] = []
    text = visible_markdown(text)
    headings = list(HEADING_RE.finditer(text))
    for index, heading in enumerate(headings):
        if not heading_predicate(heading.group(2)):
            continue
        level = len(heading.group(1))
        end = len(text)
        for next_index, next_heading in enumerate(
            headings[index + 1 :], start=index + 1
        ):
            is_boundary_level = len(next_heading.group(1)) <= level
            is_owned_subsection = heading_is_task_subsection(
                next_heading.group(2)
            ) and heading_owns_matching_tasks(text, headings, next_index)
            if is_boundary_level and not is_owned_subsection:
                end = next_heading.start()
                break
        tasks.extend(parse_tasks(text[heading.end() : end]))
    return tasks


def positive_issue(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise SystemExit(f"{label} must be a positive issue number")
    return value


def validate_unique_issue_ownership(
    path: Path, issue_map: dict[str, tuple[Owner, ...]]
) -> None:
    issue_owners: dict[int, str] = {}
    for change, owners in issue_map.items():
        for owner in owners:
            previous = issue_owners.get(owner.issue)
            if previous is not None:
                raise SystemExit(
                    f"{path} issue #{owner.issue} is owned by both {previous} and {change}"
                )
            issue_owners[owner.issue] = change


def load_issue_map(path: str | Path) -> dict[str, tuple[Owner, ...]]:
    path = Path(path)
    with path.open(encoding="utf-8") as handle:
        payload = json.load(handle)
    if payload.get("schema_version") != 2:
        raise SystemExit(f"{path} must use schema_version 2")
    changes = payload.get("changes", {})
    if not isinstance(changes, dict):
        raise SystemExit(f"{path} must contain a changes object")

    mapped: dict[str, tuple[Owner, ...]] = {}
    for change, value in changes.items():
        if isinstance(value, int) and not isinstance(value, bool):
            mapped[str(change)] = (Owner(positive_issue(value, str(change))),)
            continue
        if not isinstance(value, dict):
            raise SystemExit(f"{path} mapping for {change} must be an issue or object")
        if value.get("contract", "checklist-v1") != "checklist-v1":
            raise SystemExit(f"{path} mapping for {change} must use checklist-v1")
        owners = value.get("owners")
        if not isinstance(owners, list) or not owners:
            raise SystemExit(f"{path} mapping for {change} must contain owners")
        parsed: list[Owner] = []
        for index, owner in enumerate(owners):
            if not isinstance(owner, dict):
                raise SystemExit(f"{path} owner {change}[{index}] must be an object")
            first = owner.get("first_task")
            last = owner.get("last_task")
            if not isinstance(first, str) or not isinstance(last, str):
                raise SystemExit(
                    f"{path} owner {change}[{index}] must declare first_task and last_task"
                )
            parsed.append(
                Owner(
                    positive_issue(owner.get("issue"), f"{change}[{index}].issue"),
                    first,
                    last,
                )
            )
        primary = positive_issue(value.get("primary_issue"), f"{change}.primary_issue")
        if parsed[0].issue != primary:
            raise SystemExit(f"{path} primary issue for {change} must own the first range")
        mapped[str(change)] = tuple(parsed)

    changes_dir = path.parent / "changes"
    if changes_dir.exists():
        missing = sorted(
            child.name
            for child in changes_dir.iterdir()
            if child.is_dir() and (child / "tasks.md").exists() and child.name not in mapped
        )
        if missing:
            raise SystemExit(
                f"{path} is missing OpenSpec issue mappings for: {', '.join(missing)}"
            )
    validate_unique_issue_ownership(path, mapped)
    return mapped


def issue_payload(repo: str, number: int) -> dict[str, object]:
    payload = gh_json(
        [
            "issue",
            "view",
            str(number),
            "-R",
            repo,
            "--json",
            "body,title,state,url,number,labels,milestone",
        ]
    )
    if not isinstance(payload, dict):
        raise SystemExit(f"GitHub issue #{number} did not return an object")
    return payload


def open_issue_payloads(repo: str) -> list[dict[str, object]]:
    """Fetch the bounded open-issue set used by the vocabulary gate."""

    payload = gh_json(
        [
            "issue",
            "list",
            "-R",
            repo,
            "--state",
            "open",
            "--limit",
            "1000",
            "--json",
            "body,state,number,labels,milestone",
        ]
    )
    if not isinstance(payload, list) or not all(
        isinstance(issue, dict) for issue in payload
    ):
        raise SystemExit("GitHub open issue list did not return issue objects")
    return payload


def check_open_issue_complexity(repo: str) -> list[str]:
    failures: list[str] = []
    for issue in open_issue_payloads(repo):
        number = positive_issue(issue.get("number"), "issue number")
        failures.extend(
            f"#{number} issue contract {failure}"
            for failure in complexity_label_failures(issue)
        )
    return failures


def issue_task_headings(
    issue: dict[str, object],
) -> tuple[str, list[re.Match[str]], list[re.Match[str]], list[re.Match[str]]]:
    body = issue.get("body", "")
    if not isinstance(body, str):
        raise SystemExit("GitHub issue body must be text")
    visible_body = visible_markdown(body)
    headings = list(HEADING_RE.finditer(visible_body))
    implementation = [
        heading
        for heading in headings
        if heading_matches_implementation_tasks(heading.group(2))
    ]
    acceptance = [
        heading
        for heading in headings
        if heading_matches_acceptance_tasks(heading.group(2))
    ]
    legacy = [
        heading
        for heading in headings
        if heading_matches_openspec_tasks(heading.group(2))
    ]
    return visible_body, implementation, acceptance, legacy


def issue_uses_new_contract(issue: dict[str, object]) -> bool:
    _, implementation, acceptance, _ = issue_task_headings(issue)
    return bool(implementation or acceptance)


def issue_checklist_tasks(issue: dict[str, object]) -> list[tuple[bool, str]]:
    visible_body, implementation, acceptance, legacy = issue_task_headings(issue)
    state = str(issue.get("state", "")).upper()
    if state == "OPEN":
        if len(implementation) != 1:
            raise SystemExit(
                "GitHub open issue must contain exactly one visible Implementation Tasks heading"
            )
        if legacy:
            raise SystemExit(
                "GitHub open issue must not retain a legacy OpenSpec task heading"
            )
        return parse_section_tasks(visible_body, heading_matches_implementation_tasks)
    if implementation or acceptance:
        if len(implementation) != 1 or len(acceptance) != 1 or legacy:
            raise SystemExit(
                "GitHub new-contract issue must contain exactly one visible Implementation Tasks "
                "and Acceptance and Review Tasks heading"
            )
        return parse_section_tasks(visible_body, heading_matches_implementation_tasks)
    if len(legacy) != 1:
        raise SystemExit(
            "GitHub closed historical issue must contain exactly one visible legacy OpenSpec task heading"
        )
    return parse_section_tasks(visible_body, heading_matches_openspec_tasks)


def acceptance_review_tasks(issue: dict[str, object]) -> list[tuple[bool, str]]:
    visible_body, _, acceptance, _ = issue_task_headings(issue)
    if len(acceptance) != 1:
        raise SystemExit(
            "GitHub issue must contain exactly one visible Acceptance and Review Tasks heading"
        )
    return parse_section_tasks(visible_body, heading_matches_acceptance_tasks)


def heading_section(
    text: str, headings: list[re.Match[str]], index: int
) -> str:
    """Return one heading's visible body through the next same-or-higher heading."""

    heading = headings[index]
    end = len(text)
    for later in headings[index + 1 :]:
        if len(later.group(1)) <= len(heading.group(1)):
            end = later.start()
            break
    return text[heading.end() : end].strip()


def named_markdown_section(text: str, name: str) -> str | None:
    """Return one uniquely named visible Markdown section."""

    visible = visible_markdown(text)
    headings = list(HEADING_RE.finditer(visible))
    matches = [
        index
        for index, heading in enumerate(headings)
        if clean(heading.group(2)).casefold() == name.casefold()
    ]
    if len(matches) != 1:
        return None
    return heading_section(visible, headings, matches[0])


def required_markdown_section_failures(
    text: str, names: set[str], document: str
) -> list[str]:
    """Require one non-empty visible section for every named heading."""

    visible = visible_markdown(text)
    headings = list(HEADING_RE.finditer(visible))
    failures: list[str] = []
    for name in sorted(names):
        matches = [
            index
            for index, heading in enumerate(headings)
            if clean(heading.group(2)).casefold() == name
        ]
        if len(matches) != 1:
            failures.append(
                f"{document} section {name!r} must appear exactly once; found {len(matches)}"
            )
            continue
        body = heading_section(visible, headings, matches[0])
        if not clean(HEADING_RE.sub("", body)):
            failures.append(f"{document} section {name!r} must not be empty")
    return failures


def openspec_readiness_failures(root: Path, change: str) -> list[str]:
    """Validate the bounded artifacts required before milestone planning."""

    change_root = root / "openspec" / "changes" / change
    failures: list[str] = []
    documents: dict[str, str] = {}
    for name in ("proposal.md", "design.md"):
        path = change_root / name
        try:
            documents[name] = path.read_text(encoding="utf-8")
        except OSError:
            failures.append(f"{change} is not implementation-ready: missing readable {name}")
    proposal = documents.get("proposal.md")
    if proposal is not None:
        failures.extend(
            f"{change} {failure}"
            for failure in required_markdown_section_failures(
                proposal, REQUIRED_PROPOSAL_HEADINGS, "proposal"
            )
        )
    design = documents.get("design.md")
    if design is not None:
        failures.extend(
            f"{change} {failure}"
            for failure in required_markdown_section_failures(
                design, REQUIRED_DESIGN_HEADINGS, "design"
            )
        )
        dependencies = named_markdown_section(design, "dependencies / cross-issue impact")
        if dependencies is not None and not (
            re.search(r"#\d+", dependencies)
            or ARCHITECTURE_NA_RE.fullmatch(dependencies.strip())
        ):
            failures.append(
                f"{change} design must name cross-issue dependencies or use a reasoned N/A"
            )
        open_questions = named_markdown_section(design, "open questions")
        if open_questions is not None and clean(open_questions).casefold() not in {
            "none",
            "none.",
        }:
            failures.append(f"{change} still has unresolved open questions")
    specs_root = change_root / "specs"
    specs = sorted(specs_root.glob("*/spec.md")) if specs_root.is_dir() else []
    if not specs:
        failures.append(f"{change} is not implementation-ready: no delta spec")
    for spec in specs:
        try:
            text = visible_markdown(spec.read_text(encoding="utf-8"))
        except OSError:
            failures.append(f"{change} has an unreadable delta spec: {spec}")
            continue
        if re.search(r"(?m)^### Requirement:\s+\S", text) is None:
            failures.append(f"{change} delta spec has no requirement: {spec}")
        if re.search(r"(?m)^#### Scenario:\s+\S", text) is None:
            failures.append(f"{change} delta spec has no scenario: {spec}")
    try:
        _, tasks = local_tasks(root, change)
    except SystemExit as error:
        failures.append(str(error))
    else:
        contract_tasks = [task for task in tasks if task_id(task).startswith("1.")]
        if not contract_tasks or any(not checked for checked, _ in contract_tasks):
            failures.append(
                f"{change} contract/specification tasks must be present and checked before milestone planning"
            )
    return failures


def planned_issue_failures(
    issue: dict[str, object],
    issue_map: dict[str, tuple[Owner, ...]],
    root: Path,
) -> list[str]:
    """Validate one open issue when it is assigned to a release milestone."""

    if str(issue.get("state", "")).upper() != "OPEN":
        return []
    milestone = issue.get("milestone")
    if not isinstance(milestone, dict):
        return []
    title = milestone.get("title")
    if not isinstance(title, str) or RELEASE_MILESTONE_RE.fullmatch(title) is None:
        return []
    number = positive_issue(issue.get("number"), "issue number")
    issue_to_change = {
        owner.issue: change for change, owners in issue_map.items() for owner in owners
    }
    failures: list[str] = []
    change = issue_to_change.get(number)
    if change is None:
        failures.append(
            f"#{number} cannot be planned in {title}: no local OpenSpec mapping"
        )
        return failures
    labels = issue.get("labels")
    status_labels = {
        label.get("name")
        for label in (labels if isinstance(labels, list) else [])
        if isinstance(label, dict)
        and isinstance(label.get("name"), str)
        and str(label.get("name")).startswith("status:")
    }
    if status_labels != {"status:ready"}:
        failures.append(
            f"#{number} cannot be planned in {title}: expected only status:ready, found "
            f"{', '.join(sorted(status_labels)) or 'no status label'}"
        )
    failures.extend(openspec_readiness_failures(root, change))
    return failures


def architecture_diagram_link_failures(section: str, repo: str, root: Path) -> list[str]:
    """Validate durable local architecture-document links for one issue section."""

    if ARCHITECTURE_NA_RE.fullmatch(section.strip()):
        return []
    if re.match(r"(?is)^\s*N/?A\b", section):
        return [
            "'architecture diagrams' N/A decision must use 'N/A: <reason>' with a non-empty reason"
        ]
    matches = list(MARKDOWN_LINK_RE.finditer(section))
    urls = [match.group(1) for match in matches]
    if not urls:
        return [
            "'architecture diagrams' section must contain at least one Markdown HTTPS link"
        ]
    expected_owner, expected_repo = repo_parts(repo)
    docs_root = (root / "docs").resolve()
    failures: list[str] = []
    if "](" in MARKDOWN_LINK_RE.sub("", section):
        failures.append(
            "'architecture diagrams' section contains an unsupported or malformed inline Markdown link"
        )
    for url in urls:
        parsed = urlsplit(url)
        segments = [unquote(segment) for segment in parsed.path.split("/") if segment]
        if (
            parsed.scheme != "https"
            or parsed.netloc.casefold() != "github.com"
            or parsed.query
            or len(segments) < 6
            or segments[0].casefold() != expected_owner.casefold()
            or segments[1].casefold() != expected_repo.casefold()
        ):
            failures.append(
                f"architecture diagram link {url!r} must target repository {repo!r} over HTTPS"
            )
            continue
        if segments[2:5] != ["blob", "main", "docs"]:
            failures.append(
                f"architecture diagram link {url!r} must use /blob/main/docs/"
            )
            continue
        relative_parts = segments[5:]
        if any(
            part in {".", ".."}
            or "/" in part
            or "\\" in part
            or ":" in part
            for part in relative_parts
        ):
            failures.append(
                f"architecture diagram link {url!r} contains an unsafe documentation path"
            )
            continue
        if len(relative_parts) != 1:
            failures.append(
                f"architecture diagram link {url!r} must target one direct document under /blob/main/docs/"
            )
            continue
        candidate = docs_root.joinpath(*relative_parts).resolve()
        try:
            candidate.relative_to(docs_root)
        except ValueError:
            failures.append(
                f"architecture diagram link {url!r} escapes the local docs directory"
            )
            continue
        if candidate.suffix.casefold() != ".md":
            failures.append(
                f"architecture diagram link {url!r} must target a Markdown document"
            )
        elif not candidate.is_file():
            failures.append(
                f"architecture diagram link {url!r} has no matching local documentation file"
            )
        elif not parsed.fragment:
            failures.append(
                f"architecture diagram link {url!r} must include a Markdown heading fragment"
            )
        else:
            try:
                document = candidate.read_text(encoding="utf-8")
            except OSError as error:
                failures.append(
                    f"architecture diagram link {url!r} documentation could not be read: {error}"
                )
                continue
            fragment = unquote(parsed.fragment)
            if not fragment.startswith(GITHUB_RENDERED_HEADING_PREFIX):
                failures.append(
                    f"architecture diagram link {url!r} must use the browser-native "
                    f"#{GITHUB_RENDERED_HEADING_PREFIX}<heading> fragment"
                )
                continue
            heading_fragment = fragment.removeprefix(GITHUB_RENDERED_HEADING_PREFIX)
            diagram_section = markdown_heading_section(document, heading_fragment)
            if diagram_section is None:
                failures.append(
                    f"architecture diagram link {url!r} has no matching GitHub-style heading fragment"
                )
            elif not contains_mermaid_diagram(diagram_section):
                failures.append(
                    f"architecture diagram link {url!r} must target a section containing a non-empty "
                    "fenced Mermaid block accepted by the locked syntax parser"
                )
    return failures


def complexity_label_failures(issue: dict[str, object]) -> list[str]:
    """Validate the singular vocabulary-only complexity label on open issues."""

    if str(issue.get("state", "")).upper() != "OPEN":
        return []
    labels = issue.get("labels")
    complexity = [
        label.get("name")
        for label in (labels if isinstance(labels, list) else [])
        if isinstance(label, dict)
        and isinstance(label.get("name"), str)
        and str(label["name"]).startswith("complexity:")
    ]
    invalid = sorted(set(complexity) - COMPLEXITY_LABELS)
    if invalid:
        return [
            "open issue complexity labels must use exactly one of "
            f"{', '.join(sorted(COMPLEXITY_LABELS))}; found unknown "
            f"{', '.join(invalid)}"
        ]
    if len(complexity) != 1:
        found = ", ".join(complexity) if complexity else "none"
        return [
            "open issue must carry exactly one complexity label from "
            f"{', '.join(sorted(COMPLEXITY_LABELS))}; found {found}"
        ]
    return []


def acceptance_state_failures(
    expected_tasks: list[tuple[bool, str]],
    acceptance_tasks: list[tuple[bool, str]],
    *,
    require_complete: bool,
) -> list[str]:
    failures: list[str] = []
    if any(not checked for checked, _ in expected_tasks):
        if any(checked for checked, _ in acceptance_tasks):
            failures.append(
                "acceptance and review tasks must be unchecked while implementation tasks are incomplete"
            )
        if require_complete:
            failures.append(
                "closed or release-complete issue must have both implementation and acceptance tasks checked"
            )
        return failures
    saw_unchecked = False
    for index, (checked, _) in enumerate(acceptance_tasks, start=1):
        if not checked:
            saw_unchecked = True
        elif saw_unchecked:
            failures.append(
                f"acceptance and review task {index} is checked after an unchecked task"
            )
    if require_complete and (
        any(not checked for checked, _ in expected_tasks)
        or any(not checked for checked, _ in acceptance_tasks)
    ):
        failures.append(
            "closed or release-complete issue must have both implementation and acceptance tasks checked"
        )
    return failures


def issue_contract_failures(
    issue: dict[str, object],
    expected_tasks: list[tuple[bool, str]],
    repo: str,
    root: Path,
) -> list[str]:
    """Validate the two-list #305 issue shape and its state transition."""

    state = str(issue.get("state", "")).upper()
    if state != "OPEN":
        if not issue_uses_new_contract(issue):
            return []
        try:
            implementation = issue_checklist_tasks(issue)
            acceptance = acceptance_review_tasks(issue)
        except SystemExit as error:
            return [str(error)]
        failures = []
        if implementation != expected_tasks:
            failures.append(
                f"implementation section does not mirror expected tasks: "
                f"{first_task_difference(expected_tasks, implementation)}"
            )
        failures.extend(
            acceptance_task_failures(acceptance, require_complete=True)
        )
        failures.extend(
            acceptance_state_failures(
                expected_tasks, acceptance, require_complete=True
            )
        )
        return failures
    body = issue.get("body", "")
    if not isinstance(body, str):
        return ["body is not text"]
    visible_body, implementation_headings, acceptance_headings, legacy_headings = (
        issue_task_headings(issue)
    )
    failures: list[str] = []
    if requires_exact_head_proof(visible_body):
        failures.append(
            "must bind proof to behavior-relevant inputs instead of exact-head commit identity"
        )
    if legacy_headings:
        failures.append(
            "open mapped issues must use Implementation Tasks, not a legacy OpenSpec task heading"
        )
    positions: list[int] = []
    headings = list(HEADING_RE.finditer(visible_body))
    normalized = [clean(heading.group(2)).casefold() for heading in headings]
    for required in REQUIRED_OPEN_ISSUE_HEADINGS:
        matches = [index for index, value in enumerate(normalized) if value == required]
        if len(matches) != 1:
            failures.append(
                f"must contain exactly one visible non-empty {required!r} section"
            )
            continue
        index = matches[0]
        positions.append(index)
        if not heading_section(visible_body, headings, index):
            failures.append(f"{required!r} section must not be empty")
    if len(implementation_headings) != 1:
        failures.append(
            "must contain exactly one visible non-empty 'implementation tasks' section"
        )
    else:
        positions.append(
            next(
                index
                for index, heading in enumerate(headings)
                if heading.start() == implementation_headings[0].start()
            )
        )
    if len(acceptance_headings) != 1:
        failures.append(
            "must contain exactly one visible non-empty 'acceptance and review tasks' section"
        )
    else:
        positions.append(
            next(
                index
                for index, heading in enumerate(headings)
                if heading.start() == acceptance_headings[0].start()
            )
        )
    if len(positions) == len(REQUIRED_OPEN_ISSUE_HEADINGS) + 2 and positions != sorted(
        positions
    ):
        failures.append("required issue sections must follow the #305 order")

    architecture_indexes = [
        index for index, value in enumerate(normalized) if value == "architecture diagrams"
    ]
    if len(architecture_indexes) == 1:
        architecture_section = heading_section(
            visible_body, headings, architecture_indexes[0]
        )
        failures.extend(
            architecture_diagram_link_failures(architecture_section, repo, root)
        )

    try:
        acceptance = acceptance_review_tasks(issue)
    except SystemExit as error:
        acceptance = []
        failures.append(str(error))
    failures.extend(acceptance_task_failures(acceptance, require_complete=False))
    failures.extend(
        acceptance_state_failures(
            expected_tasks, acceptance, require_complete=False
        )
    )

    premortem_indexes = [
        index for index, value in enumerate(normalized) if value == "pre-mortem"
    ]
    if len(premortem_indexes) != 1:
        return failures
    premortem = heading_section(visible_body, headings, premortem_indexes[0])
    marker = re.search(r"(?mi)^Likely failure modes:\s*$", premortem)
    mitigation_marker = re.search(r"(?mi)^Mitigations:\s*$", premortem)
    if marker is None or mitigation_marker is None or marker.start() >= mitigation_marker.start():
        failures.append(
            "pre-mortem must contain ordered 'Likely failure modes:' and 'Mitigations:' blocks"
        )
        return failures
    likely_failures = premortem[marker.end() : mitigation_marker.start()]
    if (
        re.search(
            rf"(?m)^[ ]{{0,3}}{UNORDERED_LIST_MARKER_RE}\s+\S", likely_failures
        )
        is None
    ):
        failures.append("pre-mortem must list at least one likely failure mode")

    mitigation_text = premortem[mitigation_marker.end() :]
    mitigation_matches = list(MITIGATION_RE.finditer(mitigation_text))
    mitigation_tasks = parse_tasks(mitigation_text)
    if not mitigation_matches:
        failures.append("pre-mortem must list at least one mitigation checkbox")
        return failures
    if len(mitigation_matches) != len(mitigation_tasks):
        failures.append(
            "every pre-mortem mitigation checkbox must end with "
            "'(Implementation tasks: <task ids>)'"
        )
    expected_by_id = {task_id(task): task[0] for task in expected_tasks}
    for match in mitigation_matches:
        checked = match.group(1).lower() == "x"
        kind = match.group(3)
        references = [value.strip() for value in match.group(4).split(",")]
        if kind != "Implementation":
            failures.append(
                f"mitigation {clean(match.group(2))!r} must use "
                "'(Implementation tasks: ...)' for open mapped issues"
            )
        if len(references) != len(set(references)):
            failures.append(
                f"mitigation {clean(match.group(2))!r} repeats an implementation task ID"
            )
            continue
        unknown = [value for value in references if value not in expected_by_id]
        if unknown:
            failures.append(
                f"mitigation {clean(match.group(2))!r} references unknown or foreign "
                f"implementation tasks: {', '.join(unknown)}"
            )
            continue
        should_be_checked = all(expected_by_id[value] for value in references)
        if checked != should_be_checked:
            state = "checked" if should_be_checked else "unchecked"
            failures.append(
                f"mitigation {clean(match.group(2))!r} must be {state} because of "
                f"implementation tasks {', '.join(references)}"
            )
    return failures


def acceptance_task_failures(
    acceptance: list[tuple[bool, str]], *, require_complete: bool
) -> list[str]:
    failures: list[str] = []
    if len(acceptance) != len(ACCEPTANCE_REVIEW_TASKS):
        failures.append(
            "Acceptance and Review Tasks must contain exactly five checkboxes"
        )
        return failures
    for index, ((_, actual), expected) in enumerate(
        zip(acceptance, ACCEPTANCE_REVIEW_TASKS, strict=True), start=1
    ):
        if actual != expected:
            failures.append(
                f"acceptance and review task {index} must use the canonical text"
            )
    if require_complete and any(not checked for checked, _ in acceptance):
        failures.append("closed or release-complete issue must have all acceptance tasks checked")
    return failures


def local_tasks(root: Path, change: str) -> tuple[Path, list[tuple[bool, str]]]:
    path = root / "openspec" / "changes" / change / "tasks.md"
    if not path.exists():
        raise SystemExit(f"OpenSpec tasks file missing for {change}: {path}")
    tasks = parse_tasks(path.read_text(encoding="utf-8"))
    if not tasks:
        raise SystemExit(f"OpenSpec tasks file has no checkbox tasks: {path}")
    ids = [task_id(task) for task in tasks]
    if len(ids) != len(set(ids)):
        raise SystemExit(f"OpenSpec tasks file has duplicate task identifiers: {path}")
    return path, tasks


def owner_slices(
    path: Path, tasks: list[tuple[bool, str]], owners: tuple[Owner, ...]
) -> list[tuple[Owner, list[tuple[bool, str]]]]:
    if len(owners) == 1 and owners[0].first_task is None:
        return [(owners[0], tasks)]
    positions = {task_id(task): index for index, task in enumerate(tasks)}
    slices: list[tuple[Owner, list[tuple[bool, str]]]] = []
    next_index = 0
    for owner in owners:
        if owner.first_task not in positions or owner.last_task not in positions:
            raise SystemExit(
                f"{path} owner #{owner.issue} references an unknown task range "
                f"{owner.first_task}..{owner.last_task}"
            )
        first = positions[owner.first_task]
        last = positions[owner.last_task]
        if first != next_index or last < first:
            raise SystemExit(
                f"{path} owner ranges must be ordered, disjoint, and gap-free"
            )
        slices.append((owner, tasks[first : last + 1]))
        next_index = last + 1
    if next_index != len(tasks):
        raise SystemExit(f"{path} owner ranges do not cover every task")
    return slices


def first_task_difference(
    expected: list[tuple[bool, str]], actual: list[tuple[bool, str]]
) -> str:
    for index, (left, right) in enumerate(zip(expected, actual, strict=False), start=1):
        if left != right:
            return f"task {index} differs: expected {left!r}, found {right!r}"
    return f"task count differs: expected {len(expected)}, found {len(actual)}"


def check_openspec_tasks(
    repo: str,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    planned_issue: int | None = None,
) -> list[str]:
    failures: list[str] = []
    for change, owners in sorted(issue_map.items()):
        if planned_issue is not None and all(
            owner.issue != planned_issue for owner in owners
        ):
            continue
        path, tasks = local_tasks(root, change)
        for owner, expected in owner_slices(path, tasks, owners):
            if planned_issue is not None and owner.issue != planned_issue:
                continue
            payload = issue_payload(repo, owner.issue)
            try:
                remote = issue_checklist_tasks(payload)
            except SystemExit as error:
                failures.append(f"#{owner.issue} issue contract {error}")
                continue
            print(
                f"#{owner.issue} {change}: local {len(expected)} / "
                f"remote {len(remote)} / checked "
                f"{sum(1 for checked, _ in remote if checked)}"
            )
            if remote != expected:
                failures.append(
                    f"#{owner.issue} does not exactly mirror {path}: "
                    f"{first_task_difference(expected, remote)}"
                )
            for failure in issue_contract_failures(payload, expected, repo, root):
                failures.append(f"#{owner.issue} issue contract {failure}")
            if payload.get("state") == "CLOSED" and any(
                not checked for checked, _ in remote
            ):
                failures.append(f"#{owner.issue} is closed but still has unchecked tasks")
    return failures


def repo_parts(repo: str) -> tuple[str, str]:
    parts = repo.split("/", 1)
    if len(parts) != 2 or not parts[0] or not parts[1]:
        raise SystemExit(f"--repo must be OWNER/REPO, got {repo!r}")
    return parts[0], parts[1]


def flatten_paginated_response(payload: object) -> list[dict[str, object]]:
    if not isinstance(payload, list):
        raise SystemExit("expected GitHub API pagination response to be a JSON list")
    flattened = [item for page in payload for item in page] if all(
        isinstance(page, list) for page in payload
    ) else payload
    if not all(isinstance(item, dict) for item in flattened):
        raise SystemExit("expected GitHub API pagination items to be objects")
    return flattened


def milestone_number(repo: str, milestone: str) -> int | None:
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            f"repos/{owner}/{name}/milestones",
            "--method",
            "GET",
            "-F",
            "state=all",
            "-F",
            "per_page=100",
        ]
    )
    matches = [
        item
        for item in flatten_paginated_response(payload)
        if item.get("title") == milestone
    ]
    return positive_issue(matches[0].get("number"), "milestone number") if matches else None


def milestone_issues(repo: str, milestone: str) -> list[dict[str, object]]:
    number = milestone_number(repo, milestone)
    if number is None:
        return []
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            f"repos/{owner}/{name}/issues",
            "--method",
            "GET",
            "-F",
            "state=all",
            "-F",
            f"milestone={number}",
            "-F",
            "per_page=100",
        ]
    )
    return [item for item in flatten_paginated_response(payload) if "pull_request" not in item]


def mapped_issue_numbers(issue_map: dict[str, tuple[Owner, ...]]) -> set[int]:
    return {owner.issue for owners in issue_map.values() for owner in owners}


def milestone_issue_failures(
    milestone: str, issues: list[dict[str, object]], mapped_issues: set[int]
) -> list[str]:
    failures: list[str] = []
    for item in issues:
        number = positive_issue(item.get("number"), "issue number")
        if number not in mapped_issues:
            failures.append(
                f"#{number} in milestone {milestone} has no local OpenSpec mapping"
            )
        state = str(item.get("state", "")).upper()
        if state != "CLOSED":
            failures.append(
                f"#{number} in milestone {milestone} is {state or 'UNKNOWN'}, not CLOSED"
            )
    return failures


def check_milestone_complete(
    repo: str, milestone: str, mapped_issues: set[int]
) -> list[str]:
    failures: list[str] = []
    issues = milestone_issues(repo, milestone)
    if not issues:
        return [f"milestone {milestone!r} has no issues"]
    failures.extend(milestone_issue_failures(milestone, issues, mapped_issues))
    for item in issues:
        number = positive_issue(item.get("number"), "issue number")
        if number not in mapped_issues:
            continue
        issue = issue_payload(repo, number)
        try:
            tasks = issue_checklist_tasks(issue)
        except SystemExit as error:
            failures.append(f"#{number} in milestone {milestone}: {error}")
            continue
        checked = sum(1 for is_checked, _ in tasks if is_checked)
        unchecked = len(tasks) - checked
        print(
            f"#{number} {issue.get('state')}: tasks {len(tasks)} / "
            f"checked {checked} / open {unchecked}"
        )
        if not tasks:
            failures.append(f"#{number} in milestone {milestone} has no visible checklist tasks")
        if unchecked:
            failures.append(f"#{number} in milestone {milestone} has {unchecked} unchecked tasks")
        if issue_uses_new_contract(issue):
            try:
                acceptance = acceptance_review_tasks(issue)
            except SystemExit as error:
                failures.append(f"#{number} in milestone {milestone}: {error}")
                continue
            failures.extend(
                f"#{number} in milestone {milestone} {failure}"
                for failure in acceptance_task_failures(acceptance, require_complete=True)
            )
            failures.extend(
                f"#{number} in milestone {milestone} {failure}"
                for failure in acceptance_state_failures(
                    tasks, acceptance, require_complete=True
                )
            )
    return failures


def self_test() -> None:
    sample = """
- [x] 1.1 Done task
  - [ ] Nested item
* [ ] 2.1 Open   task
    - [x] 9.9 Indented code task
<!--
- [x] 9.8 Commented task
-->
```md
- [x] 9.7 Fenced task
```
"""
    assert parse_tasks(sample) == [
        (True, "1.1 Done task"),
        (False, "Nested item"),
        (False, "2.1 Open task"),
    ]
    issue_body = """
## Discussion

- [ ] Random checkbox should not count

## OpenSpec Tasks

## 1. Review

- [x] 1.1 Anchored task

## 2. Implementation

- [ ] 2.1 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.

## 2. Acceptance Criteria

- [ ] A numeric sibling without matching task IDs must stop the task section

## 2026 Status

- [ ] Another random checkbox should not count
"""
    expected = [
        (True, "1.1 Anchored task"),
        (
            False,
            "2.1 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.",
        ),
    ]
    assert parse_section_tasks(issue_body, heading_matches_openspec_tasks) == expected
    expected = [
        (True, "1.1 Anchored task"),
        (False, "2.1 Finish ordinary implementation."),
    ]
    issue_contract = """
## Why
Explain the need.
## What Changes
Describe the change.
## Capabilities
Name the capability.
## Architecture Diagrams
- [System architecture](https://github.com/owner/repo/blob/main/docs/projectatlas-3-architecture.md#user-content-architecture-views)
## Release Scope
Target the release.
## Non-Goals
State exclusions.
## Pre-Mortem
Likely failure modes:
- The issue contract drifts.
Mitigations:
- [ ] Keep the contract synchronized. (Implementation tasks: 2.1)
## Implementation Tasks
## 1. Review
- [x] 1.1 Anchored task
## 2. Implementation
- [ ] 2.1 Finish ordinary implementation.
## Acceptance and Review Tasks
- [ ] Intent and outcome review: Confirm the delivered behavior solves the complete issue `Why` and `What Changes`, provides the declared capabilities and release scope, and respects the non-goals at the real user or agent boundary.
- [ ] Implementation review: Review the complete implementation for correctness, architecture and ownership, applicable Rust and database pattern fit, security, resource bounds, compatibility, and unnecessary complexity; resolve every material finding.
- [ ] Specification and architecture review: Reconcile the issue, OpenSpec requirements and tasks, source, documentation, and every required architecture diagram; add missing specifications or diagrams or record a reasoned N/A when no view is needed.
- [ ] Test and proof review: Confirm the owning unit, integration, E2E, fault, concurrency, performance, and platform tests required by the issue are sound, causally exercise real behavior, and cover positive, negative, failure, and compatibility outcomes.
- [ ] Final readiness review: Confirm every implementation task is complete, all human and automated review feedback is resolved or dispositioned, required local and hosted gates pass, and no behavior or proof boundary remains partial.
"""
    self_test_root = Path(__file__).resolve().parents[2]

    def contract_failures(
        issue: dict[str, object], tasks: list[tuple[bool, str]]
    ) -> list[str]:
        return issue_contract_failures(issue, tasks, "owner/repo", self_test_root)

    assert contract_failures({"state": "OPEN", "body": issue_contract}, expected) == []
    assert complexity_label_failures(
        {"state": "OPEN", "labels": [{"name": "complexity:medium"}]}
    ) == []
    assert any(
        "exactly one complexity label" in failure
        for failure in complexity_label_failures({"state": "OPEN", "labels": []})
    )
    assert any(
        "exactly one complexity label" in failure
        for failure in complexity_label_failures(
            {
                "state": "OPEN",
                "labels": [
                    {"name": "complexity:low"},
                    {"name": "complexity:high"},
                ],
            }
        )
    )
    assert any(
        "unknown" in failure
        for failure in complexity_label_failures(
            {"state": "OPEN", "labels": [{"name": "complexity:unknown"}]}
        )
    )
    assert complexity_label_failures(
        {"state": "OPEN", "labels": [{"name": "type:chore"}]}
    )
    assert complexity_label_failures(
        {
            "state": "OPEN",
            "number": 466,
            "body": "Unmapped backlog issue without task fields.",
            "labels": [{"name": "complexity:high"}],
        }
    ) == []
    assert complexity_label_failures(
        {"state": "CLOSED", "labels": [{"name": "complexity:unknown"}]}
    ) == []
    assert acceptance_review_tasks({"state": "OPEN", "body": issue_contract}) == [
        (False, task) for task in ACCEPTANCE_REVIEW_TASKS
    ]
    missing_acceptance = issue_contract.replace(
        f"- [ ] {ACCEPTANCE_REVIEW_TASKS[2]}\n", ""
    )
    assert any(
        "exactly five checkboxes" in failure
        for failure in contract_failures({"state": "OPEN", "body": missing_acceptance}, expected)
    )
    weakened_acceptance = issue_contract.replace(
        "Intent and outcome review:", "Outcome review:"
    )
    assert any(
        "canonical text" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": weakened_acceptance}, expected
        )
    )
    extra_acceptance = issue_contract.replace(
        "## Acceptance and Review Tasks\n",
        "## Acceptance and Review Tasks\n- [ ] Extra review checkbox.\n",
    )
    assert any(
        "exactly five checkboxes" in failure
        for failure in contract_failures({"state": "OPEN", "body": extra_acceptance}, expected)
    )
    duplicate_implementation = issue_contract.replace(
        "## Acceptance and Review Tasks",
        "## Implementation Tasks\n- [ ] 9.9 Duplicate field.\n## Acceptance and Review Tasks",
    )
    assert any(
        "exactly one visible non-empty 'implementation tasks'" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": duplicate_implementation}, expected
        )
    )
    legacy_open = issue_contract.replace(
        "(Implementation tasks: 2.1)", "(OpenSpec tasks: 2.1)"
    ).replace("## Implementation Tasks", "## OpenSpec Tasks")
    assert any(
        "must use Implementation Tasks" in failure
        for failure in contract_failures({"state": "OPEN", "body": legacy_open}, expected)
    )
    hidden_duplicate_fields = issue_contract.replace(
        "## Implementation Tasks",
        "<!--\n## Implementation Tasks\n- [ ] 9.9 Hidden implementation field.\n-->\n"
        "## Implementation Tasks",
    ).replace(
        "## Acceptance and Review Tasks",
        "<!--\n## Acceptance and Review Tasks\n- [ ] Hidden acceptance field.\n-->\n"
        "## Acceptance and Review Tasks",
    )
    assert contract_failures(
        {"state": "OPEN", "body": hidden_duplicate_fields}, expected
    ) == []
    hidden_implementation = issue_contract.replace(
        "## Implementation Tasks\n", "<!--\n## Implementation Tasks\n", 1
    ).replace(
        "## Acceptance and Review Tasks\n",
        "-->\n## Acceptance and Review Tasks\n",
        1,
    )
    assert any(
        "exactly one visible non-empty 'implementation tasks'" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": hidden_implementation}, expected
        )
    )
    historical_closed = (
        "## OpenSpec Tasks\n- [x] 1.1 Anchored task\n"
        "- [x] 2.1 Historical task.\n"
    )
    assert issue_checklist_tasks({"state": "CLOSED", "body": historical_closed}) == [
        (True, "1.1 Anchored task"),
        (True, "2.1 Historical task."),
    ]
    assert contract_failures(
        {"state": "CLOSED", "body": historical_closed},
        [(True, "1.1 Anchored task"), (True, "2.1 Historical task.")],
    ) == []
    assert any(
        "both implementation and acceptance tasks checked" in failure
        for failure in contract_failures({"state": "CLOSED", "body": issue_contract}, expected)
    )
    na_contract = issue_contract.replace(
        "- [System architecture](https://github.com/owner/repo/blob/main/docs/projectatlas-3-architecture.md#user-content-architecture-views)",
        "N/A: This change has no architecture impact.",
    )
    assert contract_failures({"state": "OPEN", "body": na_contract}, expected) == []
    unexplained_na = na_contract.replace(
        "N/A: This change has no architecture impact.", "N/A"
    )
    assert any(
        "N/A decision" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": unexplained_na}, expected
        )
    )
    invalid_na = na_contract.replace(
        "N/A: This change has no architecture impact.",
        "NA: This change has no architecture impact.",
    )
    assert any(
        "N/A decision" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": invalid_na}, expected
        )
    )
    invalid_na_delimiter = na_contract.replace(
        "N/A: This change has no architecture impact.",
        "N/A - This change has no architecture impact.",
    )
    assert any(
        "N/A decision" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": invalid_na_delimiter}, expected
        )
    )
    assert contains_mermaid_diagram("```mermaid\nflowchart LR\nA --> B\n```")
    assert contains_mermaid_diagram(
        "```mermaid\n---\ntitle: Typed graph\n---\nerDiagram\nA ||--o{ B : owns\n```"
    )
    assert contains_mermaid_diagram(
        "```mermaid\nkanban\n  column1[Backlog]\n    task1[Add feature]\n```"
    )
    assert not contains_mermaid_diagram(
        "```mermaid\nflowchart LR\nthis is not valid mermaid ???\n```"
    )
    assert not contains_mermaid_diagram("```mermaid\nThis is only prose.\n```")
    assert not contains_mermaid_diagram("```mermaid\nflowchart LR\n```")
    assert not contains_mermaid_diagram("```mermaid\n---\ntitle: Missing close\nflowchart LR\n```")
    assert not contains_mermaid_diagram("```python\nflowchart LR\n```")
    assert not contains_mermaid_diagram("```mermaid\n```")
    with tempfile.TemporaryDirectory() as temporary:
        architecture_root = Path(temporary)
        docs = architecture_root / "docs"
        docs.mkdir()
        architecture = docs / "architecture.md"
        link = (
            "[Target](https://github.com/owner/repo/blob/main/docs/"
            "architecture.md#user-content-target-view)"
        )
        architecture.write_text(
            "## Target View\n\n```mermaid\n%% comment\nflowchart LR\nA --> B\n```\n"
            "\n## Later View\n\n```mermaid\nflowchart LR\nB --> C\n```\n",
            encoding="utf-8",
        )
        assert architecture_diagram_link_failures(link, "owner/repo", architecture_root) == []
        shortened_fragment = link.replace("#user-content-", "#")
        assert any(
            "browser-native #user-content-<heading> fragment" in failure
            for failure in architecture_diagram_link_failures(
                shortened_fragment, "owner/repo", architecture_root
            )
        )
        architecture.write_text(
            "## Target View\n\nArchitecture prose only.\n"
            "\n## Later View\n\n```mermaid\nflowchart LR\nB --> C\n```\n",
            encoding="utf-8",
        )
        assert any(
            "fenced Mermaid block" in failure
            for failure in architecture_diagram_link_failures(
                link, "owner/repo", architecture_root
            )
        )
        architecture.write_text(
            "## Target View\n\n```mermaid\nflowchart LR\nA --> B\n",
            encoding="utf-8",
        )
        assert any(
            "fenced Mermaid block" in failure
            for failure in architecture_diagram_link_failures(
                link, "owner/repo", architecture_root
            )
        )
    exact_head_contract = issue_contract.replace(
        "Explain the need.", "Require exact-head proof."
    )
    assert any(
        "behavior-relevant inputs" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": exact_head_contract}, expected
        )
    )
    no_exact_head_contract = issue_contract.replace(
        "Explain the need.", "Do not require exact-head proof."
    )
    assert (
        contract_failures(
            {"state": "OPEN", "body": no_exact_head_contract}, expected
        )
        == []
    )
    for negative_modal in [
        "Proof is not required at exact-head.",
        "No exact-head proof is required.",
        "Require input-bound proof rather than exact-head proof.",
        "Proof must be independent of exact-head identity.",
        "Proof must not be exact-head.",
        "Proof must not require exact-head identity.",
        "Proof should not require exact-head identity.",
        "Proof cannot require exact-head identity.",
        "Proof does not require exact-head identity.",
        "Exact-head proof is not mandatory.",
        "Exact-head proof is not allowed.",
        "Exact-head proof is not permitted.",
        "Exact-head proof need not be rerun.",
        "Proof need not require exact-head identity.",
    ]:
        assert not requires_exact_head_proof(negative_modal)
    assert not requires_exact_head_proof(
        "[Architecture](https://example.invalid/design#exact-head-proof)"
    )
    assert requires_exact_head_proof(
        "Do not use stale proof; require exact-head proof."
    )
    assert requires_exact_head_proof(
        "Proof does not require exact-head identity and exact-head evidence "
        "is required before release."
    )
    assert requires_exact_head_proof(
        "Exact-head proof is needed before release."
    )
    assert requires_exact_head_proof(
        "Proof does not require and instead enforces exact-head identity."
    )
    assert not requires_exact_head_proof(
        "Proof does not require or enforce exact-head identity."
    )
    assert not requires_exact_head_proof(
        "We do not need exact-head proof."
    )
    assert not requires_exact_head_proof(
        "Do not require commit receipts or bind releases to exact-head proof."
    )
    assert not requires_exact_head_proof(
        "Proof does not require and does not enforce exact-head identity."
    )
    assert requires_exact_head_proof(
        "Proof does not require stale-SHA identity and the release gate "
        "enforces exact-head identity."
    )
    for mandatory_requirement in [
        "Exact-head commit identity is mandatory before release.",
        "The release shall use exact-head commit identity.",
        "Only exact-head commit identity is allowed.",
        "Release is permitted only with exact-head commit identity.",
    ]:
        assert requires_exact_head_proof(mandatory_requirement)
    assert not requires_exact_head_proof(
        "The release shall not allow exact-head commit identity."
    )
    assert not requires_exact_head_proof(
        "Exact-head proof caused unnecessary reruns."
    )
    assert not requires_exact_head_proof(
        "We replace exact-head proof with input-based reuse."
    )
    punctuation_heading_contract = issue_contract.replace(
        "#architecture-views", "#sqlite-wal-durability-and-checkpoint-flow"
    )
    assert contract_failures(
        {"state": "OPEN", "body": punctuation_heading_contract}, expected
    ) == []
    assert markdown_heading_fragments(
        "## Repeated heading\n## Repeated heading\n## Repeated heading-1\n"
    ) == {"repeated-heading", "repeated-heading-1", "repeated-heading-1-1"}
    plus_marker_contract = issue_contract.replace(
        "- [ ] Keep the contract synchronized.",
        "+ [ ] Keep the contract synchronized.",
    )
    assert contract_failures({"state": "OPEN", "body": plus_marker_contract}, expected) == []
    unbound_plus_mitigation = issue_contract.replace(
        "## Implementation Tasks",
        "+ [ ] Unbound mitigation\n## Implementation Tasks",
    )
    assert any(
        "every pre-mortem mitigation checkbox" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": unbound_plus_mitigation}, expected
        )
    )
    premature = issue_contract.replace(
        "- [ ] Keep the contract synchronized.",
        "- [x] Keep the contract synchronized.",
    )
    assert any(
        "must be unchecked" in failure
        for failure in contract_failures({"state": "OPEN", "body": premature}, expected)
    )
    implemented_contract = issue_contract.replace(
        "- [ ] 2.1 Finish ordinary implementation.",
        "- [x] 2.1 Finish ordinary implementation.",
    ).replace(
        "- [ ] Keep the contract synchronized. (Implementation tasks: 2.1)",
        "- [x] Keep the contract synchronized. (Implementation tasks: 2.1)",
    )
    prefix_contract = implemented_contract.replace(
        f"- [ ] {ACCEPTANCE_REVIEW_TASKS[0]}",
        f"- [x] {ACCEPTANCE_REVIEW_TASKS[0]}",
    )
    assert contract_failures(
        {"state": "OPEN", "body": prefix_contract},
        [(True, "1.1 Anchored task"), (True, "2.1 Finish ordinary implementation.")],
    ) == []
    non_prefix_contract = prefix_contract.replace(
        f"- [ ] {ACCEPTANCE_REVIEW_TASKS[1]}",
        f"- [x] {ACCEPTANCE_REVIEW_TASKS[1]}",
    ).replace(
        f"- [x] {ACCEPTANCE_REVIEW_TASKS[0]}",
        f"- [ ] {ACCEPTANCE_REVIEW_TASKS[0]}",
    )
    assert any(
        "after an unchecked task" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": non_prefix_contract},
            [(True, "1.1 Anchored task"), (True, "2.1 Finish ordinary implementation.")],
        )
    )
    completed_contract = issue_contract.replace(
        "- [ ] Keep the contract synchronized.",
        "- [x] Keep the contract synchronized.",
    )
    completed_contract = completed_contract.replace(
        "- [ ] 2.1 Finish ordinary implementation.",
        "- [x] 2.1 Finish ordinary implementation.",
    )
    for acceptance_task in ACCEPTANCE_REVIEW_TASKS:
        completed_contract = completed_contract.replace(
            f"- [ ] {acceptance_task}", f"- [x] {acceptance_task}"
        )
    assert contract_failures(
        {"state": "OPEN", "body": completed_contract},
        [
            (True, "1.1 Anchored task"),
            (True, "2.1 Finish ordinary implementation."),
        ],
    ) == []
    missing_scope = issue_contract.replace("## Release Scope", "## Delivery")
    assert any(
        "'release scope'" in failure
        for failure in contract_failures({"state": "OPEN", "body": missing_scope}, expected)
    )
    unknown_task = issue_contract.replace(
        "(Implementation tasks: 2.1)", "(Implementation tasks: 9.9)"
    )
    assert any(
        "unknown or foreign" in failure
        for failure in contract_failures({"state": "OPEN", "body": unknown_task}, expected)
    )
    assert contract_failures({"state": "CLOSED", "body": ""}, expected) == []
    wrong_final = [expected[0], (False, "2.1 Finish ordinary tests.")]
    assert not any(
        "final OpenSpec task must be the architecture acceptance task" in failure
        for failure in contract_failures({"state": "OPEN", "body": issue_contract}, wrong_final)
    )
    missing_architecture_link = issue_contract.replace(
        "- [System architecture](https://github.com/owner/repo/blob/main/docs/projectatlas-3-architecture.md#user-content-architecture-views)",
        "Architecture will be documented later.",
    )
    assert any(
        "at least one Markdown HTTPS link" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": missing_architecture_link}, expected
        )
    )
    for invalid_link in (
        "[Relative architecture](../AGENTS.md)",
        '[Titled relative architecture](../AGENTS.md "local copy")',
        "[Insecure architecture](http://github.com/owner/repo/blob/main/docs/projectatlas-3-architecture.md)",
        "[Mail architecture](mailto:architecture@example.com)",
        "[Nested [architecture]](../AGENTS.md)",
    ):
        mixed_architecture_links = issue_contract.replace(
            "## Release Scope", f"{invalid_link}\n## Release Scope"
        )
        assert any(
            "must target repository" in failure
            for failure in contract_failures(
                {"state": "OPEN", "body": mixed_architecture_links}, expected
            )
        )
    foreign_architecture = issue_contract.replace("owner/repo", "other/repo")
    assert any(
        "must target repository" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": foreign_architecture}, expected
        )
    )
    legacy_dev_architecture = issue_contract.replace("/blob/main/", "/blob/dev/")
    assert any(
        "must use /blob/main/docs/" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": legacy_dev_architecture}, expected
        )
    )
    sha_architecture = issue_contract.replace(
        "/blob/main/", "/blob/0123456789abcdef0123456789abcdef01234567/"
    )
    assert any(
        "must use /blob/main/docs/" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": sha_architecture}, expected
        )
    )
    traversing_architecture = issue_contract.replace(
        "docs/projectatlas-3-architecture.md", "docs/../AGENTS.md"
    )
    assert any(
        "unsafe documentation path" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": traversing_architecture}, expected
        )
    )
    non_markdown_architecture = issue_contract.replace(
        "projectatlas-3-architecture.md", "projectatlas-3-architecture.png"
    )
    assert any(
        "must target a Markdown document" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": non_markdown_architecture}, expected
        )
    )
    nested_architecture = issue_contract.replace(
        "docs/projectatlas-3-architecture.md",
        "docs/benchmarks/large-application-token-savings.md",
    )
    assert any(
        "one direct document" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": nested_architecture}, expected
        )
    )
    missing_architecture = issue_contract.replace(
        "projectatlas-3-architecture.md", "missing-architecture.md"
    )
    assert any(
        "no matching local documentation file" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": missing_architecture}, expected
        )
    )
    missing_architecture_fragment = issue_contract.replace(
        "#user-content-architecture-views", "#user-content-missing-architecture-view"
    )
    assert any(
        "no matching GitHub-style heading fragment" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": missing_architecture_fragment}, expected
        )
    )
    fragmentless_architecture = issue_contract.replace(
        "#user-content-architecture-views", ""
    )
    assert any(
        "must include a Markdown heading fragment" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": fragmentless_architecture}, expected
        )
    )
    duplicate_architecture = issue_contract.replace(
        "## Release Scope",
        "## Architecture Diagrams\n- [Second view](https://github.com/owner/repo/blob/main/docs/agent-navigation.md#user-content-initial-task-discovery)\n## Release Scope",
    )
    assert any(
        "exactly one visible non-empty 'architecture diagrams' section" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": duplicate_architecture}, expected
        )
    )
    empty_architecture = issue_contract.replace(
        "## Architecture Diagrams\n- [System architecture](https://github.com/owner/repo/blob/main/docs/projectatlas-3-architecture.md#user-content-architecture-views)\n",
        "## Architecture Diagrams\n",
    )
    assert any(
        "'architecture diagrams' section must not be empty" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": empty_architecture}, expected
        )
    )
    wrong_architecture_order = issue_contract.replace(
        "## Capabilities\nName the capability.\n## Architecture Diagrams\n- [System architecture](https://github.com/owner/repo/blob/main/docs/projectatlas-3-architecture.md#user-content-architecture-views)\n",
        "## Architecture Diagrams\n- [System architecture](https://github.com/owner/repo/blob/main/docs/projectatlas-3-architecture.md#user-content-architecture-views)\n## Capabilities\nName the capability.\n",
    )
    assert any(
        "required issue sections must follow the #305 order" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": wrong_architecture_order}, expected
        )
    )
    hidden_issue = """
```md
## OpenSpec Tasks
- [x] 9.9 Hidden fenced task
```
<!--
## OpenSpec Tasks
- [x] 9.8 Hidden commented task
-->
"""
    assert parse_section_tasks(hidden_issue, heading_matches_openspec_tasks) == []
    assert owner_slices(
        Path("tasks.md"),
        expected,
        (Owner(1, "1.1", "1.1"), Owner(2, "2.1", "2.1")),
    ) == [(Owner(1, "1.1", "1.1"), expected[:1]), (Owner(2, "2.1", "2.1"), expected[1:])]
    try:
        owner_slices(
            Path("tasks.md"),
            expected,
            (Owner(1, "1.1", "2.1"), Owner(2, "2.1", "2.1")),
        )
    except SystemExit as error:
        assert "ordered, disjoint, and gap-free" in str(error)
    else:
        raise AssertionError("overlapping owner ranges were accepted")
    assert first_task_difference(expected, expected[:-1]) == "task count differs: expected 2, found 1"
    assert repo_parts("owner/repo") == ("owner", "repo")
    assert flatten_paginated_response([[{"number": 1}], [{"number": 2}]]) == [
        {"number": 1},
        {"number": 2},
    ]
    assert mapped_issue_numbers({"one": (Owner(1),), "two": (Owner(2),)}) == {
        1,
        2,
    }
    assert milestone_issue_failures(
        "v1.0.0-00",
        [{"number": 1, "state": "closed"}, {"number": 3, "state": "open"}],
        {1, 2},
    ) == [
        "#3 in milestone v1.0.0-00 has no local OpenSpec mapping",
        "#3 in milestone v1.0.0-00 is OPEN, not CLOSED",
    ]
    saved_milestone_issues = globals()["milestone_issues"]
    saved_issue_payload = globals()["issue_payload"]
    try:
        globals()["milestone_issues"] = lambda _repo, _milestone: [
            {"number": 448, "state": "closed"}
        ]
        globals()["issue_payload"] = lambda _repo, _number: {
            "number": 448,
            "state": "CLOSED",
            "body": completed_contract,
        }
        assert check_milestone_complete("owner/repo", "v1.0.0-00", {448}) == []
        globals()["issue_payload"] = lambda _repo, _number: {
            "number": 448,
            "state": "CLOSED",
            "body": issue_contract,
        }
        assert any(
            "unchecked tasks" in failure
            for failure in check_milestone_complete(
                "owner/repo", "v1.0.0-00", {448}
            )
        )
    finally:
        globals()["milestone_issues"] = saved_milestone_issues
        globals()["issue_payload"] = saved_issue_payload
    try:
        validate_unique_issue_ownership(
            Path("issue-map.json"),
            {"one": (Owner(1),), "two": (Owner(1),)},
        )
    except SystemExit as error:
        assert "owned by both one and two" in str(error)
    else:
        raise AssertionError("duplicate issue ownership was accepted")
    try:
        validate_unique_issue_ownership(
            Path("issue-map.json"),
            {"one": (Owner(1, "1.1", "1.1"), Owner(1, "1.2", "1.2"))},
        )
    except SystemExit as error:
        assert "owned by both one and one" in str(error)
    else:
        raise AssertionError("repeated issue ownership within one change was accepted")
    ready_issue = {
        "number": 448,
        "state": "OPEN",
        "milestone": {"title": "v6.7.8-00"},
        "labels": [{"name": "status:ready"}],
    }
    backlog_issue = {**ready_issue, "labels": [{"name": "status:backlog"}]}
    assert any(
        "expected only status:ready" in failure
        for failure in planned_issue_failures(
            backlog_issue, {"missing-readiness-change": (Owner(448),)}, self_test_root
        )
    )
    assert any(
        "no local OpenSpec mapping" in failure
        for failure in planned_issue_failures(ready_issue, {}, self_test_root)
    )
    assert any(
        "missing readable proposal.md" in failure
        for failure in openspec_readiness_failures(
            self_test_root, "missing-readiness-change"
        )
    )
    with tempfile.TemporaryDirectory() as temporary:
        readiness_root = Path(temporary)
        change = readiness_root / "openspec" / "changes" / "ready-change"
        (change / "specs" / "capability").mkdir(parents=True)
        (change / "proposal.md").write_text(
            "## Why\nNeed it.\n## What Changes\nChange it.\n"
            "## Capabilities\nOne.\n## Impact\nBounded.\n",
            encoding="utf-8",
        )
        design = (
            "## Goals / Non-Goals\nGoal.\n## Decisions\nDecide.\n"
            "## Risks / Trade-offs\nRisk.\n## Migration Plan\nMigrate.\n"
            "## Dependencies / Cross-Issue Impact\nIssue #123 owns the input.\n"
            "## Open Questions\nNone.\n"
        )
        (change / "design.md").write_text(design, encoding="utf-8")
        (change / "specs" / "capability" / "spec.md").write_text(
            "## ADDED Requirements\n### Requirement: Ready contract\nIt SHALL work.\n"
            "#### Scenario: Positive\n- **WHEN** ready\n- **THEN** proceed\n",
            encoding="utf-8",
        )
        (change / "tasks.md").write_text(
            "## 1. Contract\n- [x] 1.1 Specify the contract.\n"
            "## 2. Acceptance\n- [ ] 2.1 Review the final implementation against the "
            "architecture diagrams, update the diagrams or implementation until they "
            "agree, or reconfirm the reasoned N/A.\n",
            encoding="utf-8",
        )
        assert openspec_readiness_failures(readiness_root, "ready-change") == []
        assert planned_issue_failures(
            ready_issue, {"ready-change": (Owner(448),)}, readiness_root
        ) == []
        empty_proposal = (
            "## Why\n\n## What Changes\nChange it.\n"
            "## Capabilities\nOne.\n## Impact\nBounded.\n"
        )
        (change / "proposal.md").write_text(empty_proposal, encoding="utf-8")
        assert any(
            "proposal section 'why' must not be empty" in failure
            for failure in openspec_readiness_failures(
                readiness_root, "ready-change"
            )
        )
        (change / "proposal.md").write_text(
            "## Why\nNeed it.\n## Why\nNeed it twice.\n"
            "## What Changes\nChange it.\n## Capabilities\nOne.\n"
            "## Impact\nBounded.\n",
            encoding="utf-8",
        )
        assert any(
            "proposal section 'why' must appear exactly once" in failure
            for failure in openspec_readiness_failures(
                readiness_root, "ready-change"
            )
        )
        (change / "proposal.md").write_text(
            "## Why\nNeed it.\n## What Changes\nChange it.\n"
            "## Capabilities\nOne.\n## Impact\nBounded.\n",
            encoding="utf-8",
        )
        (change / "design.md").write_text(
            design.replace("None.", "Which owner is responsible?"), encoding="utf-8"
        )
        assert any(
            "unresolved open questions" in failure
            for failure in openspec_readiness_failures(
                readiness_root, "ready-change"
            )
        )
        (change / "design.md").write_text(
            design.replace(
                "Issue #123 owns the input.", "Dependencies will be decided later."
            ),
            encoding="utf-8",
        )
        assert any(
            "must name cross-issue dependencies" in failure
            for failure in openspec_readiness_failures(
                readiness_root, "ready-change"
            )
        )
        (change / "design.md").write_text(design, encoding="utf-8")
        (change / "tasks.md").write_text(
            "## 1. Contract\n- [ ] 1.1 Specify the contract.\n",
            encoding="utf-8",
        )
        assert any(
            "contract/specification tasks" in failure
            for failure in openspec_readiness_failures(
                readiness_root, "ready-change"
            )
        )
    print("issue checklist self-test passed")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="")
    parser.add_argument("--root", default=".")
    parser.add_argument("--issue-map", default="openspec/issue-map.json")
    parser.add_argument("--milestone", action="append", default=[])
    parser.add_argument("--planned-issue", type=int)
    parser.add_argument("--skip-openspec", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if not args.repo:
        raise SystemExit("--repo is required unless --self-test is used")

    root = Path(args.root)
    failures: list[str] = []
    issue_map = load_issue_map(args.issue_map)
    failures.extend(check_open_issue_complexity(args.repo))
    if not args.skip_openspec:
        failures.extend(
            check_openspec_tasks(
                args.repo, root, issue_map, planned_issue=args.planned_issue
            )
        )
    if args.planned_issue is not None:
        failures.extend(
            planned_issue_failures(
                issue_payload(args.repo, args.planned_issue), issue_map, root
            )
        )
    mapped_issues = mapped_issue_numbers(issue_map)
    for milestone in args.milestone:
        failures.extend(check_milestone_complete(args.repo, milestone, mapped_issues))

    if failures:
        print("\nIssue checklist validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
