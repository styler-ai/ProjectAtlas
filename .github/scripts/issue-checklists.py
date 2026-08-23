"""Verify GitHub issue checklists mirror OpenSpec tasks."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from urllib.parse import unquote, urlsplit


UNORDERED_LIST_MARKER_RE = r"[-*+]"
TASK_RE = re.compile(
    rf"(?m)^[ ]{{0,3}}{UNORDERED_LIST_MARKER_RE}\s+\[([ xX])\]\s+(.+?)\s*$"
)
TASK_ID_RE = re.compile(r"^(\d+(?:\.\d+)*)\s+")
ISSUE_KEY_RE = re.compile(r"^[1-9][0-9]*$")
HEADING_RE = re.compile(r"(?m)^(#{1,6})\s+(.+?)\s*$")
TASK_SECTION_HEADING_RE = re.compile(r"^(\d+(?:\.\d+)*)\.\s+")
HTML_COMMENT_RE = re.compile(r"(?s)<!--.*?(?:-->|$)")
FENCE_RE = re.compile(r"^[ ]{0,3}(`{3,}|~{3,})")
ARCHITECTURE_NA_RE = re.compile(r"(?is)^N/A:\s*(\S(?:.*\S)?)$")
GITHUB_RENDERED_HEADING_PREFIX = "user-content-"
ARCHITECTURE_ACCEPTANCE_TASK = (
    "Review the final implementation against the architecture diagrams, update the "
    "diagrams or implementation until they agree, or reconfirm the reasoned N/A."
)
MARKDOWN_LINK_RE = re.compile(
    r"\[(?:[^\[\]\n]|\[[^\[\]\n]*\])+\]"
    r"\(\s*<?([^)>\s]+)>?"
    r"(?:\s+(?:\"[^\"\n]*\"|'[^'\n]*'|\([^\)\n]*\)))?\s*\)"
)
MITIGATION_RE = re.compile(
    rf"(?mi)^[ ]{{0,3}}{UNORDERED_LIST_MARKER_RE}\s+\[([ xX])\]\s+(.+?)\s+"
    r"\(OpenSpec tasks:\s*(\d+(?:\.\d+)*(?:\s*,\s*\d+(?:\.\d+)*)*)\)\s*$"
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
RELATES_TO_MARKER_RE = re.compile(r"(?i)\bRelates[ \t]+to[ \t]+#")
RELATES_TO_LINE_RE = re.compile(
    r"(?im)^[ \t]*Relates[ \t]+to[ \t]+#([1-9][0-9]*)(?=[ \t]*(?:[.!?]|$))"
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


@dataclass(frozen=True)
class ReleaseGraph:
    """One milestone's declared direct issue dependencies."""

    milestone: str
    release_issue: int
    blocked_by: dict[int, tuple[int, ...]]


@dataclass(frozen=True)
class PublishedSnapshot:
    """The exact default-branch revision proved before reading local artifacts."""

    branch: str
    sha: str


NATIVE_RELATION_KINDS = {"blocked_by", "sub_issue"}
NATIVE_RELATION_OPERATIONS = {"add", "remove"}
ISSUEOPS_WORKFLOW_PATH = ".github/workflows/issueops.yml"
IMPLEMENTATION_STATUS_CONTEXT = "issueops-implementation"
MERGE_AUTHORIZATION_STATUS_CONTEXT = "issueops-merge-authorized"
DISPATCH_RELATIONSHIP_EVENT = "issueops_relationship"
DISPATCH_MERGE_EVENT = "issueops_merge"
REQUEST_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{7,63}$")
MAX_COLLECTION_PAGES = 4
MAX_MILESTONES = 256
MAX_MILESTONE_ISSUES = 256
MAX_NATIVE_RELATIONS = 256
MAX_PULL_REQUEST_REFERENCES = 32
MAX_REPAIR_DEPENDENTS = 256
MAX_REPOSITORY_COLLABORATORS = 1
MAX_CHECKS = 256
MAX_REVIEWS = 100


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


def validate_request_id(value: object) -> str:
    if not isinstance(value, str) or REQUEST_ID_RE.fullmatch(value) is None:
        raise SystemExit(
            "repository_dispatch request_id must be 8-64 ASCII letters, digits, "
            "periods, underscores, or hyphens, beginning with a letter or digit"
        )
    return value


def validate_merge_request(
    pull_request_number: object, expected_head: object
) -> tuple[int, str]:
    try:
        number = positive_issue(int(pull_request_number), "merge pull request number")
    except (TypeError, ValueError):
        raise SystemExit("merge authorization requires a positive pull request number")
    if not isinstance(expected_head, str) or re.fullmatch(
        r"[0-9a-fA-F]{40}", expected_head
    ) is None:
        raise SystemExit("merge authorization requires an exact expected head SHA")
    return number, expected_head


def dispatch_outcome(event: str, request_id: str, outcome: str) -> str:
    request_id = validate_request_id(request_id)
    if outcome not in {"applied", "already-satisfied", "failed"}:
        raise SystemExit(f"unknown {event} dispatch outcome {outcome!r}")
    return json.dumps(
        {"event": event, "outcome": outcome, "request_id": request_id},
        sort_keys=True,
        separators=(",", ":"),
    )


def relationship_outcome(request_id: str, outcome: str) -> str:
    return dispatch_outcome(DISPATCH_RELATIONSHIP_EVENT, request_id, outcome)


def merge_outcome(request_id: str, outcome: str) -> str:
    return dispatch_outcome(DISPATCH_MERGE_EVENT, request_id, outcome)


def validate_native_relationship_request(
    relation_kind: object,
    operation: object,
    issue: object,
    related_issue: object,
) -> tuple[int, int]:
    if relation_kind not in NATIVE_RELATION_KINDS:
        raise SystemExit(f"native relation kind is invalid: {relation_kind!r}")
    if operation not in NATIVE_RELATION_OPERATIONS:
        raise SystemExit(f"native relation operation is invalid: {operation!r}")

    def parse_number(value: object, label: str) -> int:
        if isinstance(value, str) and re.fullmatch(r"[1-9][0-9]*", value):
            value = int(value)
        return positive_issue(value, label)

    issue_number = parse_number(issue, "issue number")
    related_number = parse_number(related_issue, "related issue number")
    if issue_number == related_number:
        raise SystemExit("native relationship cannot relate an issue to itself")
    return issue_number, related_number


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


def _blocked_by_values(value: object, label: str, path: Path) -> tuple[int, ...]:
    """Read one graph entry, accepting the compact list and object forms."""

    if isinstance(value, dict):
        value = value.get("blocked_by")
    if not isinstance(value, list):
        raise SystemExit(f"{path} {label} must declare a blocked_by array")
    parsed = tuple(positive_issue(item, f"{label}.blocked_by") for item in value)
    if len(parsed) != len(set(parsed)):
        raise SystemExit(f"{path} {label}.blocked_by contains duplicate issue numbers")
    return parsed


def parse_release_graphs(
    payload: object,
    path: str | Path,
    issue_map: dict[str, tuple[Owner, ...]],
) -> dict[str, ReleaseGraph]:
    """Validate and parse optional milestone-level release dependency graphs."""

    path = Path(path)
    if not isinstance(payload, dict):
        raise SystemExit(f"{path} must contain a JSON object")
    if "release_graphs" not in payload:
        return {}
    raw_graphs = payload["release_graphs"]
    if not isinstance(raw_graphs, dict):
        raise SystemExit(f"{path} release_graphs must be an object")
    mapped = mapped_issue_numbers(issue_map)
    graphs: dict[str, ReleaseGraph] = {}
    graph_issue_milestones: dict[int, str] = {}
    for milestone, raw_graph in raw_graphs.items():
        if not isinstance(milestone, str) or not milestone:
            raise SystemExit(f"{path} release_graphs keys must be milestone titles")
        if RELEASE_MILESTONE_RE.fullmatch(milestone) is None:
            raise SystemExit(
                f"{path} release_graphs.{milestone} must use a vMAJOR.MINOR.PATCH-00 milestone title"
            )
        if not isinstance(raw_graph, dict):
            raise SystemExit(f"{path} release_graphs.{milestone} must be an object")
        release_issue = positive_issue(
            raw_graph.get("release_issue"), f"release_graphs.{milestone}.release_issue"
        )
        raw_issues = raw_graph.get("issues")
        if not isinstance(raw_issues, dict) or not raw_issues:
            raise SystemExit(f"{path} release_graphs.{milestone} must contain issues")
        parsed: dict[int, tuple[int, ...]] = {}
        for raw_issue, value in raw_issues.items():
            if not isinstance(raw_issue, str) or ISSUE_KEY_RE.fullmatch(raw_issue) is None:
                raise SystemExit(
                    f"{path} release_graphs.{milestone}.issues keys must be issue numbers"
                )
            issue = positive_issue(int(raw_issue), f"release_graphs.{milestone}.issues.{raw_issue}")
            if issue in parsed:
                raise SystemExit(f"{path} release_graphs.{milestone} repeats issue #{issue}")
            previous_milestone = graph_issue_milestones.get(issue)
            if previous_milestone is not None:
                raise SystemExit(
                    f"{path} issue #{issue} appears in release graphs for both "
                    f"{previous_milestone} and {milestone}"
                )
            if issue not in mapped:
                raise SystemExit(
                    f"{path} release_graphs.{milestone} issue #{issue} has no local OpenSpec mapping"
                )
            parsed[issue] = _blocked_by_values(
                value, f"release_graphs.{milestone}.issues.{raw_issue}", path
            )
            graph_issue_milestones[issue] = milestone
        if release_issue not in parsed:
            raise SystemExit(
                f"{path} release_graphs.{milestone} release_issue #{release_issue} is not an issue"
            )
        nodes = set(parsed)
        for issue, blockers in parsed.items():
            if issue in blockers:
                raise SystemExit(
                    f"{path} release_graphs.{milestone} issue #{issue} cannot block itself"
                )
            unknown = sorted(set(blockers) - nodes)
            if unknown:
                joined = ", ".join(f"#{number}" for number in unknown)
                raise SystemExit(
                    f"{path} release_graphs.{milestone} issue #{issue} references "
                    f"unknown graph issue(s): {joined}"
                )
        expected_release_blockers = nodes - {release_issue}
        if set(parsed[release_issue]) != expected_release_blockers:
            missing = sorted(expected_release_blockers - set(parsed[release_issue]))
            extra = sorted(set(parsed[release_issue]) - expected_release_blockers)
            detail = []
            if missing:
                detail.append("missing " + ", ".join(f"#{number}" for number in missing))
            if extra:
                detail.append("extra " + ", ".join(f"#{number}" for number in extra))
            raise SystemExit(
                f"{path} release_graphs.{milestone} release_issue #{release_issue} "
                "must be directly blocked by every other graph issue ("
                + "; ".join(detail)
                + ")"
            )
        # Edges point from an issue to its blockers; a topological peel catches every cycle.
        remaining = {issue: set(blockers) for issue, blockers in parsed.items()}
        while remaining:
            leaves = {issue for issue, blockers in remaining.items() if not blockers}
            if not leaves:
                cycle = ", ".join(f"#{issue}" for issue in sorted(remaining))
                raise SystemExit(
                    f"{path} release_graphs.{milestone} contains a dependency cycle: {cycle}"
                )
            for issue in leaves:
                remaining.pop(issue)
            for blockers in remaining.values():
                blockers.difference_update(leaves)
        graphs[milestone] = ReleaseGraph(milestone, release_issue, parsed)
    return graphs


def load_release_graphs(
    path: str | Path, issue_map: dict[str, tuple[Owner, ...]]
) -> dict[str, ReleaseGraph]:
    path = Path(path)
    try:
        with path.open(encoding="utf-8") as handle:
            payload = json.load(handle)
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"{path} release graph state is unreadable: {error}") from error
    return parse_release_graphs(payload, path, issue_map)


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


def issue_checklist_tasks(issue: dict[str, object]) -> list[tuple[bool, str]]:
    body = issue.get("body", "")
    if not isinstance(body, str):
        raise SystemExit("GitHub issue body must be text")
    visible_body = visible_markdown(body)
    authoritative_headings = [
        heading
        for heading in HEADING_RE.finditer(visible_body)
        if heading_matches_openspec_tasks(heading.group(2))
    ]
    if len(authoritative_headings) != 1:
        raise SystemExit(
            "GitHub issue must contain exactly one visible OpenSpec task heading"
        )
    return parse_section_tasks(visible_body, heading_matches_openspec_tasks)


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


def issue_contract_failures(
    issue: dict[str, object],
    expected_tasks: list[tuple[bool, str]],
    repo: str,
    root: Path,
) -> list[str]:
    """Validate the concise #305 issue shape for open mapped work."""

    if issue.get("state") != "OPEN":
        return []
    body = issue.get("body", "")
    if not isinstance(body, str):
        return ["body is not text"]
    visible_body = visible_markdown(body)
    if requires_exact_head_proof(visible_body):
        return [
            "must bind proof to behavior-relevant inputs instead of exact-head commit identity"
        ]
    headings = list(HEADING_RE.finditer(visible_body))
    normalized = [clean(heading.group(2)).lower() for heading in headings]
    failures: list[str] = []
    final_task = (
        TASK_ID_RE.sub("", expected_tasks[-1][1], count=1) if expected_tasks else ""
    )
    if clean(final_task).casefold() != ARCHITECTURE_ACCEPTANCE_TASK.casefold():
        failures.append(
            "final OpenSpec task must be the architecture acceptance task: "
            f"{ARCHITECTURE_ACCEPTANCE_TASK}"
        )
    positions: list[int] = []
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
    task_positions = [
        index
        for index, heading in enumerate(headings)
        if heading_matches_openspec_tasks(heading.group(2))
    ]
    if len(task_positions) == 1:
        positions.append(task_positions[0])
    if len(positions) == len(REQUIRED_OPEN_ISSUE_HEADINGS) + 1 and positions != sorted(
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
            "'(OpenSpec tasks: <task ids>)'"
        )
    expected_by_id = {task_id(task): task[0] for task in expected_tasks}
    for match in mitigation_matches:
        checked = match.group(1).lower() == "x"
        references = [value.strip() for value in match.group(3).split(",")]
        if len(references) != len(set(references)):
            failures.append(
                f"mitigation {clean(match.group(2))!r} repeats an OpenSpec task ID"
            )
            continue
        unknown = [value for value in references if value not in expected_by_id]
        if unknown:
            failures.append(
                f"mitigation {clean(match.group(2))!r} references unknown or foreign "
                f"OpenSpec tasks: {', '.join(unknown)}"
            )
            continue
        should_be_checked = all(expected_by_id[value] for value in references)
        if checked != should_be_checked:
            state = "checked" if should_be_checked else "unchecked"
            failures.append(
                f"mitigation {clean(match.group(2))!r} must be {state} because of "
                f"OpenSpec tasks {', '.join(references)}"
            )
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
            remote = issue_checklist_tasks(payload)
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


def bounded_api_collection(
    args: list[str], cap: int, label: str
) -> list[dict[str, object]]:
    """Read a finite collection page-by-page and fail closed at cap+1."""

    page_size = min(100, cap + 1)
    collected: list[dict[str, object]] = []
    for page in range(1, MAX_COLLECTION_PAGES + 1):
        payload = gh_api_json(
            [*args, "-F", f"per_page={page_size}", "-F", f"page={page}"]
        )
        if not isinstance(payload, list) or not all(
            isinstance(item, dict) for item in payload
        ):
            raise SystemExit(f"GitHub {label} page {page} was not an object list")
        collected.extend(payload)
        if len(collected) > cap:
            raise SystemExit(f"GitHub {label} exceeded the bounded {cap}-item limit")
        if len(payload) < page_size:
            return collected
    raise SystemExit(
        f"GitHub {label} exceeded the bounded {MAX_COLLECTION_PAGES}-page limit"
    )


def bounded_api_values(args: list[str], cap: int, label: str) -> list[object]:
    """Read a bounded collection whose API items are scalar values."""

    page_size = min(100, cap + 1)
    collected: list[object] = []
    for page in range(1, MAX_COLLECTION_PAGES + 1):
        payload = gh_api_json(
            [*args, "-F", f"per_page={page_size}", "-F", f"page={page}"]
        )
        if not isinstance(payload, list):
            raise SystemExit(f"GitHub {label} page {page} was malformed")
        collected.extend(payload)
        if len(collected) > cap:
            raise SystemExit(f"GitHub {label} exceeded the bounded {cap}-item limit")
        if len(payload) < page_size:
            return collected
    raise SystemExit(
        f"GitHub {label} exceeded the bounded {MAX_COLLECTION_PAGES}-page limit"
    )


def milestone_number(repo: str, milestone: str) -> int | None:
    owner, name = repo_parts(repo)
    milestones = bounded_api_collection(
        [
            f"repos/{owner}/{name}/milestones",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-F",
            "state=all",
        ],
        MAX_MILESTONES,
        "milestones",
    )
    matches = [
        item
        for item in milestones
        if item.get("title") == milestone
    ]
    return positive_issue(matches[0].get("number"), "milestone number") if matches else None


def milestone_issues(repo: str, milestone: str) -> list[dict[str, object]]:
    number = milestone_number(repo, milestone)
    if number is None:
        return []
    owner, name = repo_parts(repo)
    issues = bounded_api_collection(
        [
            f"repos/{owner}/{name}/issues",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-F",
            "state=all",
            "-F",
            f"milestone={number}",
        ],
        MAX_MILESTONE_ISSUES,
        f"milestone {milestone} issues",
    )
    return [item for item in issues if "pull_request" not in item]


GITHUB_API_VERSION = "2026-03-10"


def native_relation_issue_number(
    repo: str, payload: object, label: str
) -> int:
    """Read a native relation only when its payload identifies the addressed repo."""

    if not isinstance(payload, dict):
        raise SystemExit(f"GitHub returned {label} as a non-object")
    owner, name = repo_parts(repo)
    expected_full_name = f"{owner}/{name}".casefold()
    identities: list[bool] = []
    repository_url = payload.get("repository_url")
    if repository_url is not None:
        if not isinstance(repository_url, str):
            raise SystemExit(f"GitHub returned {label} with malformed repository identity")
        parsed = urlsplit(repository_url)
        identities.append(
            parsed.scheme.casefold() == "https"
            and parsed.netloc.casefold() == "api.github.com"
            and parsed.path.rstrip("/").casefold()
            == f"/repos/{owner}/{name}".casefold()
            and not parsed.query
            and not parsed.fragment
        )
    repository = payload.get("repository")
    if repository is not None:
        if not isinstance(repository, dict):
            raise SystemExit(f"GitHub returned {label} with malformed repository identity")
        full_name = repository.get("full_name")
        if full_name is not None:
            if not isinstance(full_name, str):
                raise SystemExit(f"GitHub returned {label} with malformed repository identity")
            identities.append(full_name.casefold() == expected_full_name)
        nested_name = repository.get("name")
        nested_owner = repository.get("owner")
        if nested_name is not None or nested_owner is not None:
            if not isinstance(nested_name, str) or not nested_name:
                raise SystemExit(f"GitHub returned {label} with malformed repository identity")
            if not isinstance(nested_owner, dict):
                raise SystemExit(f"GitHub returned {label} with malformed repository identity")
            owner_login = nested_owner.get("login")
            if not isinstance(owner_login, str) or not owner_login:
                raise SystemExit(f"GitHub returned {label} with malformed repository identity")
            identities.append(
                f"{owner_login}/{nested_name}".casefold() == expected_full_name
            )
    if not identities or not all(identities):
        raise SystemExit(
            f"GitHub returned {label} without the exact {repo} repository identity"
        )
    return positive_issue(payload.get("number"), label)


def native_blocked_by(repo: str, issue: int) -> set[int]:
    """Read GitHub's native blocked-by edges for one issue."""

    owner, name = repo_parts(repo)
    dependencies = bounded_api_collection(
        [
            f"repos/{owner}/{name}/issues/{issue}/dependencies/blocked_by",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ],
        MAX_NATIVE_RELATIONS,
        f"blocked-by relations for #{issue}",
    )
    result: set[int] = set()
    for dependency in dependencies:
        number = native_relation_issue_number(repo, dependency, "blocked-by issue number")
        if number in result:
            raise SystemExit(f"GitHub returned duplicate blocked-by issue #{number} for #{issue}")
        result.add(number)
    return result


def native_sub_issues(repo: str, issue: int) -> set[int]:
    """Read GitHub's native sub-issue children for one issue."""

    owner, name = repo_parts(repo)
    children = bounded_api_collection(
        [
            f"repos/{owner}/{name}/issues/{issue}/sub_issues",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ],
        MAX_NATIVE_RELATIONS,
        f"sub-issues for #{issue}",
    )
    result: set[int] = set()
    for child in children:
        number = native_relation_issue_number(repo, child, "sub-issue number")
        if number in result:
            raise SystemExit(f"GitHub returned duplicate sub-issue #{number} for #{issue}")
        result.add(number)
    return result


def native_parent_issue(repo: str, issue: int) -> int | None:
    """Read a native parent, distinguishing an absent parent from API failure."""

    owner, name = repo_parts(repo)
    args = [
        "gh",
        "api",
        f"repos/{owner}/{name}/issues/{issue}/parent",
        "--method",
        "GET",
        "-H",
        f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        "--include",
    ]
    process = subprocess.run(
        args,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=120,
        check=False,
    )
    status_match = re.search(
        r"HTTP/\S+\s+(\d{3})", process.stdout + process.stderr
    )
    status = int(status_match.group(1)) if status_match else None
    if status == 404:
        return None
    if process.returncode:
        detail = process.stderr.strip() or process.stdout.strip()
        raise SystemExit(
            f"command failed ({status or 'unknown HTTP status'}): {json.dumps(args)}"
            + (f"\n{detail}" if detail else "")
        )
    body = process.stdout.rsplit("\r\n\r\n", 1)[-1].rsplit("\n\n", 1)[-1]
    try:
        payload = json.loads(body)
    except json.JSONDecodeError as error:
        raise SystemExit(f"GitHub parent response for #{issue} was not valid JSON: {error}") from error
    if not isinstance(payload, dict):
        raise SystemExit(f"GitHub parent response for #{issue} was not an object")
    return native_relation_issue_number(repo, payload, "parent issue number")


def native_issue_id(repo: str, issue: int, label: str) -> int:
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            f"repos/{owner}/{name}/issues/{positive_issue(issue, 'issue number')}",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ]
    )
    if not isinstance(payload, dict):
        raise SystemExit(f"GitHub returned {label} as a non-object")
    returned_number = native_relation_issue_number(repo, payload, label)
    if returned_number != issue:
        raise SystemExit(
            f"GitHub returned {label} #{returned_number}, expected #{issue}"
        )
    return positive_issue(payload.get("id"), f"{label} id")


def mutate_native_relationship(
    repo: str,
    issue: int,
    related_issue: int,
    relation_kind: str,
    operation: str,
    root: Path | None = None,
    published_snapshot: PublishedSnapshot | None = None,
) -> bool:
    issue, related_issue = validate_native_relationship_request(
        relation_kind, operation, issue, related_issue
    )
    read_relation = native_blocked_by if relation_kind == "blocked_by" else native_sub_issues
    current = read_relation(repo, issue)
    desired = operation == "add"
    present = related_issue in current
    changed = desired != present
    if changed:
        related_id = native_issue_id(repo, related_issue, "related issue")
        owner, name = repo_parts(repo)
        if relation_kind == "blocked_by":
            path = f"repos/{owner}/{name}/issues/{issue}/dependencies/blocked_by"
            if not desired:
                path += f"/{related_id}"
            field = "issue_id" if desired else None
        else:
            path = (
                f"repos/{owner}/{name}/issues/{issue}/sub_issues"
                if desired
                else f"repos/{owner}/{name}/issues/{issue}/sub_issue"
            )
            field = "sub_issue_id"
            if desired:
                parent = native_parent_issue(repo, related_issue)
                if parent is not None and parent != issue:
                    raise SystemExit(
                        f"related issue #{related_issue} already has native parent #{parent}"
                    )
        request = [
            "gh",
            "api",
            path,
            "--method",
            "POST" if desired else "DELETE",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ]
        if field is not None:
            request.extend(["-F", f"{field}={related_id}"])
        if root is not None:
            if published_snapshot is None:
                raise SystemExit("relationship mutation is missing its published snapshot")
            current_snapshot = require_published_snapshot(repo, root)
            if current_snapshot != published_snapshot:
                raise SystemExit(
                    "published snapshot moved before native relationship mutation"
                )
        run(request)
    actual = read_relation(repo, issue)
    if (related_issue in actual) != desired:
        raise SystemExit(
            f"native {relation_kind} {operation} read-back did not produce the requested edge"
        )
    return changed


def mutate_native_relationship_and_revalidate(
    repo: str,
    issue: int,
    related_issue: int,
    relation_kind: str,
    operation: str,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    request_id: str,
    root: Path | None = None,
    published_snapshot: PublishedSnapshot | None = None,
) -> None:
    """Mutate one edge, then validate the complete declared release graph."""

    validate_declared_native_transition(
        repo,
        issue,
        related_issue,
        relation_kind,
        operation,
        release_graphs,
    )
    if root is not None:
        if published_snapshot is None:
            raise SystemExit("relationship mutation is missing its published snapshot")
        current_snapshot = require_published_snapshot(repo, root)
        if current_snapshot != published_snapshot:
            raise SystemExit(
                "published snapshot moved before native relationship preflight"
            )
    changed = mutate_native_relationship(
        repo,
        issue,
        related_issue,
        relation_kind,
        operation,
        root=root,
        published_snapshot=published_snapshot,
    )
    if root is not None:
        try:
            current_snapshot = require_published_snapshot(repo, root)
        except BaseException as error:
            raise SystemExit(
                "native relationship mutation completed remote read-back but its "
                f"published snapshot could not be revalidated: {error}"
            ) from error
        if current_snapshot != published_snapshot:
            raise SystemExit(
                "native relationship mutation completed remote read-back but the "
                "published snapshot moved; no success outcome was emitted"
            )
    failures = release_graph_failures(repo, release_graphs, issue_map)
    if failures:
        raise SystemExit("relationship graph reconciliation failed:\n" + "\n".join(failures))
    if root is not None:
        try:
            current_snapshot = require_published_snapshot(repo, root)
        except BaseException as error:
            raise SystemExit(
                "native relationship graph was reconciled but its published snapshot "
                f"could not be revalidated: {error}"
            ) from error
        if current_snapshot != published_snapshot:
            raise SystemExit(
                "native relationship graph was reconciled against a moved published "
                "snapshot; no success outcome was emitted"
            )
    print(relationship_outcome(request_id, "applied" if changed else "already-satisfied"))


def _live_graph_failure(operation: str, error: BaseException) -> str:
    detail = str(error).strip() or error.__class__.__name__
    return f"release dependency state unreadable while {operation}: {detail}"


def release_graph_failures(
    repo: str,
    graphs: dict[str, ReleaseGraph],
    issue_map: dict[str, tuple[Owner, ...]],
    milestones: set[str] | None = None,
) -> list[str]:
    """Reconcile selected declarations with milestone membership and native edges."""

    failures: list[str] = []
    selected = (
        {milestone: graph for milestone, graph in graphs.items() if milestone in milestones}
        if milestones is not None
        else graphs
    )
    mapped_numbers = mapped_issue_numbers(issue_map)
    for milestone, graph in sorted(selected.items()):
        for issue in sorted(set(graph.blocked_by) - mapped_numbers):
            failures.append(
                f"release graph {milestone} issue #{issue} has no local OpenSpec mapping"
            )
        try:
            issues = milestone_issues(repo, milestone)
        except BaseException as error:
            failures.append(_live_graph_failure(f"reading milestone {milestone!r}", error))
            continue
        live_numbers = set()
        for item in issues:
            try:
                number = positive_issue(item.get("number"), "milestone issue number")
                if number in live_numbers:
                    failures.append(
                        f"live milestone {milestone} repeats issue #{number}; "
                        "membership is not exactly once"
                    )
                live_numbers.add(number)
            except SystemExit as error:
                failures.append(_live_graph_failure(f"reading milestone {milestone!r}", error))
                live_numbers = set()
                break
        declared_numbers = set(graph.blocked_by)
        for issue in sorted(declared_numbers - live_numbers):
            failures.append(f"release graph {milestone} issue #{issue} is not in the live milestone")
        for issue in sorted(live_numbers - declared_numbers):
            failures.append(f"live milestone {milestone} issue #{issue} is missing from release graph")
        if live_numbers != declared_numbers:
            # Native checks below would be misleading for an incomplete graph.
            continue
        expected_children = declared_numbers - {graph.release_issue}
        try:
            actual_children = native_sub_issues(repo, graph.release_issue)
        except BaseException as error:
            failures.append(
                _live_graph_failure(
                    f"reading native sub-issues for #{graph.release_issue}", error
                )
            )
        else:
            missing = sorted(expected_children - actual_children)
            extra = sorted(actual_children - expected_children)
            if missing:
                failures.append(
                    f"#{graph.release_issue} is missing native sub-issue relation(s): "
                    + ", ".join(f"#{number}" for number in missing)
                )
            if extra:
                failures.append(
                    f"#{graph.release_issue} has extra native sub-issue relation(s): "
                    + ", ".join(f"#{number}" for number in extra)
                )
        try:
            root_parent = native_parent_issue(repo, graph.release_issue)
        except BaseException as error:
            failures.append(
                _live_graph_failure(
                    f"reading native parent for #{graph.release_issue}", error
                )
            )
        else:
            if root_parent is not None:
                failures.append(
                    f"release issue #{graph.release_issue} must not have native parent #{root_parent}"
                )
        for child in sorted(expected_children):
            try:
                parent = native_parent_issue(repo, child)
            except BaseException as error:
                failures.append(
                    _live_graph_failure(f"reading native parent for #{child}", error)
                )
                continue
            if parent is None:
                failures.append(
                    f"graph child #{child} has no native parent; expected #{graph.release_issue}"
                )
            elif parent != graph.release_issue:
                failures.append(
                    f"graph child #{child} has native parent #{parent}; "
                    f"expected #{graph.release_issue}"
                )
        for issue, expected in sorted(graph.blocked_by.items()):
            try:
                actual = native_blocked_by(repo, issue)
            except BaseException as error:
                failures.append(_live_graph_failure(f"reading native blocked-by edges for #{issue}", error))
                continue
            expected_set = set(expected)
            missing = sorted(expected_set - actual)
            extra = sorted(actual - expected_set)
            if missing:
                failures.append(
                    f"#{issue} is missing native blocked-by relation(s): "
                    + ", ".join(f"#{number}" for number in missing)
                )
            if extra:
                failures.append(
                    f"#{issue} has extra native blocked-by relation(s): "
                    + ", ".join(f"#{number}" for number in extra)
                )
    return failures


def graph_for_issue(
    graphs: dict[str, ReleaseGraph], issue: int
) -> ReleaseGraph | None:
    matches = [graph for graph in graphs.values() if issue in graph.blocked_by]
    return matches[0] if matches else None


def target_graph_failures(
    issue: dict[str, object], graphs: dict[str, ReleaseGraph]
) -> tuple[ReleaseGraph | None, list[str]]:
    """Select the declaring graph before reconciling its declared milestone."""

    number = positive_issue(issue.get("number"), "issue number")
    graph = graph_for_issue(graphs, number)
    if graph is None:
        return None, []
    milestone = issue.get("milestone")
    live_title = milestone.get("title") if isinstance(milestone, dict) else None
    if live_title != graph.milestone:
        return graph, [
            f"#{number} is declared in release graph {graph.milestone} but its live "
            f"milestone is {live_title or 'unset'}"
        ]
    return graph, []


def declared_native_relations(
    graphs: dict[str, ReleaseGraph], relation_kind: str, issue: int
) -> set[int]:
    """Return the declared native relation targets for one graph-owned issue."""

    owner = declared_native_owner(graphs, relation_kind, issue)
    if relation_kind == "blocked_by":
        return set(owner.blocked_by.get(issue, ()))
    if relation_kind == "sub_issue":
        return {child for child in owner.blocked_by if child != owner.release_issue}
    raise SystemExit(f"native relation kind is invalid: {relation_kind!r}")


def declared_native_owner(
    graphs: dict[str, ReleaseGraph], relation_kind: str, issue: int
) -> ReleaseGraph:
    """Select exactly one graph owner before consulting native relation state."""

    if relation_kind == "blocked_by":
        matches = [graph for graph in graphs.values() if issue in graph.blocked_by]
    elif relation_kind == "sub_issue":
        matches = [graph for graph in graphs.values() if graph.release_issue == issue]
    else:
        raise SystemExit(f"native relation kind is invalid: {relation_kind!r}")
    if len(matches) != 1:
        raise SystemExit(
            f"native {relation_kind} source #{issue} must belong to exactly one release graph"
        )
    return matches[0]


def validate_declared_native_transition(
    repo: str,
    issue: int,
    related_issue: int,
    relation_kind: str,
    operation: str,
    release_graphs: dict[str, ReleaseGraph],
) -> None:
    """Reject graph-widening or graph-erasing mutations before their POST/DELETE."""

    owner = declared_native_owner(release_graphs, relation_kind, issue)
    declared = (
        set(owner.blocked_by.get(issue, ()))
        if relation_kind == "blocked_by"
        else {child for child in owner.blocked_by if child != owner.release_issue}
    )
    read_relation = native_blocked_by if relation_kind == "blocked_by" else native_sub_issues
    current = read_relation(repo, issue)
    present = related_issue in current
    if operation == "add":
        if related_issue not in declared:
            raise SystemExit(
                f"native {relation_kind} add for #{issue} -> #{related_issue} is not declared"
            )
        return
    if related_issue in declared:
        raise SystemExit(
            f"native {relation_kind} remove for #{issue} -> #{related_issue} would erase a declared relation"
        )
    if not present:
        raise SystemExit(
            f"native {relation_kind} remove for #{issue} -> #{related_issue} has no declared drift"
        )


def reverse_declared_dependents(
    graphs: dict[str, ReleaseGraph], blocker: int
) -> list[int]:
    """Derive bounded direct dependents from the declared release graphs only."""

    dependents = sorted(
        issue
        for graph in graphs.values()
        for issue, blockers in graph.blocked_by.items()
        if blocker in blockers
    )
    if len(dependents) > MAX_REPAIR_DEPENDENTS:
        raise SystemExit(
            "reverse release-graph dependents exceeded the bounded repair limit"
        )
    return dependents


def commit_status(repo: str, sha: str, state: str, description: str, context: str = IMPLEMENTATION_STATUS_CONTEXT) -> None:
    if re.fullmatch(r"[0-9a-fA-F]{40}", sha) is None:
        raise SystemExit("GitHub commit status requires an exact 40-character SHA")
    if state not in {"pending", "success", "failure", "error"}:
        raise SystemExit(f"invalid commit status state: {state!r}")
    if not description or len(description) > 140:
        raise SystemExit("commit status description must be 1..140 characters")
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            f"repos/{owner}/{name}/statuses/{sha}",
            "--method",
            "POST",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-f",
            f"state={state}",
            "-f",
            f"context={context}",
            "-f",
            f"description={description}",
        ]
    )
    if not isinstance(payload, dict):
        raise SystemExit("GitHub commit status response must be an object")


def pull_request_readback(
    repo: str,
    pull_request_number: int,
    expected_head_sha: str | None = None,
    expected_references: list[object] | None = None,
    expected_body: str | None = None,
) -> dict[str, object]:
    """Read one bounded PR identity before any status or auto-merge mutation."""

    number = positive_issue(pull_request_number, "pull request number")
    payload = gh_json(
        [
            "pr",
            "view",
            str(number),
            "-R",
            repo,
            "--json",
            "number,headRefOid,closingIssuesReferences,id,autoMergeRequest,milestone,body",
        ]
    )
    if not isinstance(payload, dict) or positive_issue(
        payload.get("number"), "pull request number"
    ) != number:
        raise SystemExit("GitHub pull-request identity was malformed")
    head = payload.get("headRefOid")
    if not isinstance(head, str) or re.fullmatch(r"[0-9a-fA-F]{40}", head) is None:
        raise SystemExit(f"PR #{number} did not expose an exact head SHA")
    if expected_head_sha is not None and head != expected_head_sha:
        raise SystemExit(f"PR #{number} head changed before mutation")
    references = payload.get("closingIssuesReferences")
    if not isinstance(references, list) or len(references) > MAX_PULL_REQUEST_REFERENCES:
        raise SystemExit(f"PR #{number} closing references were malformed or unbounded")
    for reference in references:
        native_relation_issue_number(repo, reference, "pull request closing issue reference")
    if expected_references is not None and references != expected_references:
        raise SystemExit(f"PR #{number} closing references changed before mutation")
    if expected_body is not None and payload.get("body") != expected_body:
        raise SystemExit(f"PR #{number} body relationship changed before mutation")
    return payload


def resolve_pull_request_owner(
    repo: str,
    details: dict[str, object],
    release_graphs: dict[str, ReleaseGraph] | None = None,
) -> tuple[int | None, ReleaseGraph | None, list[str]]:
    """Resolve one native or explicit release-root owner before readiness checks."""

    references = details.get("closingIssuesReferences")
    body = details.get("body")
    failures: list[str] = []
    relation_matches = list(RELATES_TO_LINE_RE.finditer(body)) if isinstance(body, str) else []
    relation_markers = list(RELATES_TO_MARKER_RE.finditer(body)) if isinstance(body, str) else []
    if not isinstance(body, str):
        failures.append("merge PR body was missing or malformed")
    elif len(relation_markers) != len(relation_matches):
        failures.append("merge PR body contained a malformed or non-standalone Relates to clause")
    if len(relation_matches) > 1:
        failures.append("merge PR body contained more than one Relates to owner")
    owner_issue_number: int | None = None
    graph: ReleaseGraph | None = None
    if not isinstance(references, list):
        failures.append("merge PR closing references were malformed")
    elif len(references) == 1:
        if relation_matches:
            failures.append("merge PR cannot combine a closing owner with a Relates to owner")
        try:
            owner_issue_number = native_relation_issue_number(
                repo, references[0], "merge PR closing issue reference"
            )
        except BaseException as error:
            failures.append(f"merge PR closing owner identity was unreadable: {error}")
    elif len(references) == 0:
        if len(relation_matches) != 1:
            failures.append("planning PR must contain exactly one standalone Relates to owner")
        else:
            owner_issue_number = int(relation_matches[0].group(1))
    else:
        failures.append("merge PR must have one closing owner or one Relates to owner")
        if relation_matches:
            failures.append("merge PR cannot combine multiple owners with a Relates to owner")
    if owner_issue_number is None or failures:
        return owner_issue_number, graph, failures
    if release_graphs is None:
        if not references:
            failures.append("planning PR Relates to owner requires a declared release graph")
    else:
        graph_matches = (
            [candidate for candidate in release_graphs.values() if candidate.release_issue == owner_issue_number]
            if not references
            else [candidate for candidate in release_graphs.values() if owner_issue_number in candidate.blocked_by]
        )
        if len(graph_matches) != 1:
            failures.append(
                f"merge PR owning issue #{owner_issue_number} must belong to exactly one release graph"
            )
        else:
            graph = graph_matches[0]
    try:
        owner_issue = issue_payload(repo, owner_issue_number)
    except BaseException as error:
        failures.append(f"merge PR owning issue identity or milestone was unreadable: {error}")
        return owner_issue_number, graph, failures
    owner_milestone = owner_issue.get("milestone")
    owner_title = (
        owner_milestone.get("title")
        if isinstance(owner_milestone, dict)
        else None
    )
    if not isinstance(owner_title, str) or RELEASE_MILESTONE_RE.fullmatch(owner_title) is None:
        failures.append("merge PR owning issue milestone was missing or malformed")
    if graph is not None and isinstance(owner_title, str) and graph.milestone != owner_title:
        failures.append(
            f"merge PR owning issue #{owner_issue_number} milestone does not match "
            f"release graph {graph.milestone}"
        )
    expected_title = graph.milestone if graph is not None else owner_title
    pr_milestone = details.get("milestone")
    pr_title = pr_milestone.get("title") if isinstance(pr_milestone, dict) else None
    if not isinstance(pr_title, str) or RELEASE_MILESTONE_RE.fullmatch(pr_title) is None:
        failures.append("merge PR milestone was missing or malformed")
    elif isinstance(expected_title, str) and pr_title != expected_title:
        failures.append(
            f"merge PR milestone {pr_title} does not match owning release milestone {expected_title}"
        )
    return owner_issue_number, graph, failures


def pull_request_milestone_failures(
    repo: str,
    details: dict[str, object],
    release_graphs: dict[str, ReleaseGraph] | None = None,
) -> list[str]:
    """Require one owner and an unchanged release milestone before mutation."""

    return resolve_pull_request_owner(repo, details, release_graphs)[2]


def implementation_reference_failures(
    repo: str,
    reference: object,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
) -> list[str]:
    number = native_relation_issue_number(repo, reference, "closing issue reference")
    issue = issue_payload(repo, number)
    target_graph, failures = target_graph_failures(issue, release_graphs)
    failures.extend(check_openspec_tasks(repo, root, issue_map, planned_issue=number))
    failures.extend(planned_issue_failures(issue, issue_map, root))
    failures.extend(implementation_issue_failures(repo, issue, release_graphs))
    milestone = target_graph.milestone if target_graph is not None else None
    if milestone is None:
        live_milestone = issue.get("milestone")
        milestone = live_milestone.get("title") if isinstance(live_milestone, dict) else None
    if isinstance(milestone, str):
        failures.extend(release_graph_failures(repo, release_graphs, issue_map, {milestone}))
    return failures


def publish_implementation_status_for_pr(
    repo: str,
    pull_request_number: int,
    root: Path,
    issue_map_path: str | Path,
    expected_pr_head_sha: str | None = None,
) -> None:
    pull_request = gh_json(
        [
            "pr",
            "view",
            str(positive_issue(pull_request_number, "pull request number")),
            "-R",
            repo,
            "--json",
            "number,headRefOid,closingIssuesReferences,author,milestone",
        ]
    )
    if not isinstance(pull_request, dict):
        raise SystemExit("GitHub pull-request response must be an object")
    number = positive_issue(pull_request.get("number"), "pull request number")
    sha = pull_request.get("headRefOid")
    if number != pull_request_number or not isinstance(sha, str):
        raise SystemExit("GitHub pull-request identity or head was invalid")
    expected_sha = expected_pr_head_sha or sha
    if sha != expected_sha:
        raise SystemExit(f"PR #{number} head changed before status publication")
    references = pull_request.get("closingIssuesReferences")
    if not isinstance(references, list) or len(references) > MAX_PULL_REQUEST_REFERENCES:
        raise SystemExit("pull request closing references exceeded the bounded limit")
    readback = pull_request_readback(
        repo,
        number,
        expected_head_sha=expected_sha,
        expected_references=references,
    )
    failures: list[str] = []
    release_graphs: dict[str, ReleaseGraph] | None = None
    author = pull_request.get("author")
    author_login = author.get("login") if isinstance(author, dict) else None
    if author_login == "dependabot[bot]" or not references:
        description = (
            "Dependabot dependency update uses the standard CI path"
            if author_login == "dependabot[bot]"
            else "Planning PR has no native implementation closing reference"
        )
    else:
        require_published_snapshot(repo, root)
        issue_map = load_issue_map(issue_map_path)
        release_graphs = load_release_graphs(issue_map_path, issue_map)
        failures.extend(pull_request_milestone_failures(repo, readback, release_graphs))
        for reference in references:
            try:
                failures.extend(
                    implementation_reference_failures(
                        repo, reference, root, issue_map, release_graphs
                    )
                )
            except BaseException as error:
                failures.append(str(error))
        description = (
            "Native implementation references passed"
            if not failures
            else "Native implementation references failed: " + clean(failures[0])
        )[:140]
    if failures:
        raise SystemExit("implementation status preflight failed:\n" + "\n".join(failures))
    commit_status(repo, sha, "pending", "Revalidating native implementation references")
    final_readback = pull_request_readback(
        repo,
        number,
        expected_head_sha=expected_sha,
        expected_references=references,
        expected_body=readback.get("body") if isinstance(readback.get("body"), str) else None,
    )
    if references and author_login != "dependabot[bot]":
        final_milestone_failures = pull_request_milestone_failures(
            repo, final_readback, release_graphs
        )
        if final_milestone_failures:
            commit_status(
                repo,
                expected_sha,
                "failure",
                "Implementation readiness failed after milestone re-read",
            )
            raise SystemExit(
                "implementation status final milestone read-back failed:\n"
                + "\n".join(final_milestone_failures)
            )
    state = "success" if not failures else "failure"
    commit_status(repo, expected_sha, state, description)
    if failures:
        raise SystemExit("implementation status failed:\n" + "\n".join(failures))


def revoke_merge_authorization_for_pr(
    repo: str,
    pull_request_number: int,
    expected_head_sha: str | None = None,
    expected_references: list[object] | None = None,
    release_graphs: dict[str, ReleaseGraph] | None = None,
) -> None:
    details = pull_request_readback(
        repo,
        pull_request_number,
        expected_head_sha=expected_head_sha,
        expected_references=expected_references,
    )
    number = positive_issue(details["number"], "pull request number")
    sha = details["headRefOid"]
    failures: list[str] = []
    failures.extend(pull_request_milestone_failures(repo, details, release_graphs))
    auto_merge_request = details.get("autoMergeRequest")
    if auto_merge_request is not None:
        if not isinstance(auto_merge_request, dict) or not isinstance(
            auto_merge_request.get("id"), str
        ):
            failures.append(f"PR #{number} auto-merge identity was malformed")
        elif not isinstance(details.get("id"), str) or not details["id"]:
            failures.append(f"PR #{number} pull-request node identity was malformed")
        else:
            try:
                disable_auto_merge(repo, details["id"])
            except BaseException as error:
                failures.append(f"PR #{number} auto-merge revocation failed: {error}")
    try:
        pull_request_readback(
            repo,
            number,
            expected_head_sha=sha,
            expected_references=details["closingIssuesReferences"],
            expected_body=details.get("body") if isinstance(details.get("body"), str) else None,
        )
        commit_status(
            repo,
            sha,
            "failure",
            "Merge authorization is only issued by a trusted one-shot dispatch",
            MERGE_AUTHORIZATION_STATUS_CONTEXT,
        )
    except BaseException as error:
        failures.append(f"PR #{number} merge authorization status failed: {error}")
    if failures:
        raise SystemExit("merge authorization revocation failed:\n" + "\n".join(failures))


GITHUB_ACTIONS_APP_ID = 15368


def bounded_api_object_collection(
    args: list[str], key: str, cap: int, label: str
) -> list[dict[str, object]]:
    """Read a paginated object collection with the same finite cap contract."""

    page_size = min(100, cap + 1)
    collected: list[dict[str, object]] = []
    for page in range(1, MAX_COLLECTION_PAGES + 1):
        payload = gh_api_json(
            [*args, "-F", f"per_page={page_size}", "-F", f"page={page}"]
        )
        if not isinstance(payload, dict) or not isinstance(payload.get(key), list):
            raise SystemExit(f"GitHub {label} page {page} was malformed")
        items = payload[key]
        if not all(isinstance(item, dict) for item in items):
            raise SystemExit(f"GitHub {label} page {page} contained a non-object")
        collected.extend(items)
        if len(collected) > cap:
            raise SystemExit(f"GitHub {label} exceeded the bounded {cap}-item limit")
        if len(items) < page_size:
            return collected
    raise SystemExit(
        f"GitHub {label} exceeded the bounded {MAX_COLLECTION_PAGES}-page limit"
    )


def git_output(root: Path, args: list[str]) -> str:
    """Read one bounded Git fact without allowing a candidate checkout fallback."""

    command = ["git", "-C", str(root), *args]
    try:
        process = subprocess.run(
            command,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=120,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f"published readiness Git inspection failed: {error}") from error
    if process.returncode:
        detail = process.stderr.strip()
        raise SystemExit(
            "published readiness Git inspection failed: "
            f"{detail or 'git returned a non-zero status'}"
        )
    return process.stdout.strip()


def default_branch_head(repo: str, branch: str) -> str:
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            f"repos/{owner}/{name}/git/ref/heads/{branch}",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ]
    )
    if not isinstance(payload, dict):
        raise SystemExit("default branch ref response was malformed")
    obj = payload.get("object")
    sha = obj.get("sha") if isinstance(obj, dict) else None
    if not isinstance(sha, str) or re.fullmatch(r"[0-9a-fA-F]{40}", sha) is None:
        raise SystemExit("default branch ref did not contain an exact commit SHA")
    return sha


def require_published_snapshot(repo: str, root: Path) -> PublishedSnapshot:
    """Prove that local artifact reads come from the exact clean live default branch."""

    checkout = root.resolve()
    reported_root = Path(git_output(root, ["rev-parse", "--show-toplevel"])).resolve()
    if os.path.normcase(str(reported_root)) != os.path.normcase(str(checkout)):
        raise SystemExit("published readiness root is not the addressed Git checkout")
    if git_output(root, ["status", "--porcelain=v1", "--untracked-files=no"]):
        raise SystemExit("published readiness checkout has tracked modifications")
    local_sha = git_output(root, ["rev-parse", "HEAD"])
    if re.fullmatch(r"[0-9a-fA-F]{40}", local_sha) is None:
        raise SystemExit("published readiness checkout HEAD was malformed")
    repository = gh_json(["repo", "view", repo, "--json", "defaultBranchRef"])
    branch_ref = repository.get("defaultBranchRef") if isinstance(repository, dict) else None
    branch = branch_ref.get("name") if isinstance(branch_ref, dict) else None
    if not isinstance(branch, str) or not branch:
        raise SystemExit("published readiness default branch identity was malformed")
    live_sha = default_branch_head(repo, branch)
    if local_sha.casefold() != live_sha.casefold():
        raise SystemExit("published readiness checkout HEAD does not equal live default branch")
    return PublishedSnapshot(branch, live_sha)


def required_check_contexts(repo: str, branch: str) -> list[str]:
    owner, name = repo_parts(repo)
    payload = bounded_api_values(
        [
            f"repos/{owner}/{name}/branches/{branch}/protection/required_status_checks/contexts",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ],
        MAX_CHECKS,
        "required status-check contexts",
    )
    contexts: list[str] = []
    for item in payload:
        context = item.get("context") if isinstance(item, dict) else item
        if not isinstance(context, str) or not context:
            raise SystemExit("required status-check context was malformed")
        contexts.append(context)
    return contexts


def merge_authorization_policy(repo: str, branch: str) -> int:
    """Prove the live repository gate is installed before arming auto-merge."""

    owner, name = repo_parts(repo)
    repository = gh_api_json(
        [
            f"repos/{owner}/{name}",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ]
    )
    if not isinstance(repository, dict):
        raise SystemExit("repository identity response was malformed")
    repository_name = repository.get("name")
    repository_full_name = repository.get("full_name")
    repository_owner = repository.get("owner")
    if (
        not isinstance(repository_name, str)
        or repository_name.casefold() != name.casefold()
        or not isinstance(repository_full_name, str)
        or repository_full_name.casefold() != f"{owner}/{name}".casefold()
        or not isinstance(repository_owner, dict)
    ):
        raise SystemExit("repository identity response was malformed")
    owner_login = repository_owner.get("login")
    if not isinstance(owner_login, str) or not owner_login:
        raise SystemExit("repository owner identity was missing or malformed")
    if repository_owner.get("type") != "User":
        raise SystemExit("merge authorization requires a personal User repository owner")
    if owner_login.casefold() != owner.casefold():
        raise SystemExit("repository owner did not match the addressed repository")
    collaborators = bounded_api_collection(
        [
            f"repos/{owner_login}/{repository_name}/collaborators",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-F",
            "affiliation=all",
        ],
        MAX_REPOSITORY_COLLABORATORS,
        "repository collaborators",
    )
    if len(collaborators) != 1:
        raise SystemExit(
            "merge authorization requires exactly the personal repository owner as collaborator"
        )
    collaborator = collaborators[0]
    permissions = collaborator.get("permissions")
    if (
        collaborator.get("login") != owner_login
        or collaborator.get("type") != "User"
        or not isinstance(permissions, dict)
        or permissions.get("admin") is not True
    ):
        raise SystemExit(
            "repository collaborators must contain only the matching personal owner with admin access"
        )
    protection = gh_api_json(
        [
            f"repos/{owner}/{name}/branches/{branch}/protection",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ]
    )
    if not isinstance(protection, dict):
        raise SystemExit("branch protection response was malformed")
    review_policy = protection.get("required_pull_request_reviews")
    if review_policy is None:
        required_approvals = 0
    elif not isinstance(review_policy, dict):
        raise SystemExit("branch protection pull-request review policy was malformed")
    else:
        required_approvals = review_policy.get("required_approving_review_count")
        if (
            isinstance(required_approvals, bool)
            or not isinstance(required_approvals, int)
            or required_approvals < 0
        ):
            raise SystemExit("branch protection approving-review count was malformed")
    if required_approvals > 0:
        raise SystemExit(
            "merge authorization requires branch protection with zero approving reviews"
        )
    required = protection.get("required_status_checks")
    if not isinstance(required, dict) or required.get("strict") is not True:
        raise SystemExit("branch protection is not strict")
    checks = required.get("checks")
    if not isinstance(checks, list) or len(checks) > MAX_CHECKS:
        raise SystemExit("branch protection required checks exceeded the bounded limit")
    if any(
        not isinstance(check, dict)
        or not isinstance(check.get("context"), str)
        or not isinstance(check.get("app_id"), int)
        for check in checks
    ):
        raise SystemExit("branch protection required checks were malformed")
    if not any(
        check["context"] == MERGE_AUTHORIZATION_STATUS_CONTEXT
        and check["app_id"] == GITHUB_ACTIONS_APP_ID
        for check in checks
    ):
        raise SystemExit("branch protection does not require the merge authorization check")
    repository_view = gh_json(["repo", "view", repo, "--json", "allowAutoMerge"])
    if not isinstance(repository_view, dict) or repository_view.get("allowAutoMerge") is not True:
        raise SystemExit("repository auto-merge is not enabled")
    return required_approvals


def check_run_collections(repo: str, sha: str) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    owner, name = repo_parts(repo)
    statuses = bounded_api_collection(
        [
            f"repos/{owner}/{name}/commits/{sha}/statuses",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ],
        MAX_CHECKS,
        f"commit statuses for {sha}",
    )
    checks = bounded_api_object_collection(
        [
            f"repos/{owner}/{name}/commits/{sha}/check-runs",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ],
        "check_runs",
        MAX_CHECKS,
        f"check runs for {sha}",
    )
    return statuses, checks


def pull_request_reviews(repo: str, number: int) -> list[dict[str, object]]:
    owner, name = repo_parts(repo)
    return bounded_api_collection(
        [
            f"repos/{owner}/{name}/pulls/{number}/reviews",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ],
        MAX_REVIEWS,
        f"reviews for PR #{number}",
    )


def unresolved_review_failures(repo: str, number: int) -> list[str]:
    owner, name = repo_parts(repo)
    query = """
    query($owner:String!, $repo:String!, $number:Int!) {
      repository(owner:$owner, name:$repo) {
        pullRequest(number:$number) {
          reviewThreads(first:100) {
            nodes { isResolved }
            pageInfo { hasNextPage }
          }
        }
      }
    }
    """.replace("reviewThreads(first:100)", f"reviewThreads(first:{MAX_REVIEWS})")
    payload = gh_api_json(
        [
            "graphql",
            "-f",
            f"query={query}",
            "-F",
            f"owner={owner}",
            "-F",
            f"repo={name}",
            "-F",
            f"number={number}",
        ]
    )
    try:
        threads = payload["data"]["repository"]["pullRequest"]["reviewThreads"]
        nodes = threads["nodes"]
        has_next = threads["pageInfo"]["hasNextPage"]
    except (KeyError, TypeError):
        raise SystemExit("GitHub review-thread response was malformed")
    if not isinstance(nodes, list) or not isinstance(has_next, bool):
        raise SystemExit("GitHub review-thread collection was malformed")
    if has_next or len(nodes) > MAX_REVIEWS:
        raise SystemExit("GitHub review-thread collection exceeded the bounded limit")
    return ["unresolved review thread remains"] if any(
        not isinstance(node, dict) or node.get("isResolved") is not True for node in nodes
    ) else []


def merge_readiness_failures(
    repo: str,
    number: int,
    expected_head: str,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    expected_base_sha: str | None = None,
    published_snapshot: PublishedSnapshot | None = None,
    expected_body: str | None = None,
) -> tuple[list[str], dict[str, object]]:
    if published_snapshot is None:
        published_snapshot = require_published_snapshot(repo, root)
    details = gh_json(
        [
            "pr",
            "view",
            str(number),
            "-R",
            repo,
            "--json",
            "number,state,isDraft,baseRefName,headRefOid,mergeable,mergeCommit,id,author,closingIssuesReferences,reviewDecision,milestone,body",
        ]
    )
    if not isinstance(details, dict):
        raise SystemExit("merge PR response was malformed")
    failures: list[str] = []
    if details.get("number") != number:
        failures.append("merge PR number did not match the request")
    if details.get("headRefOid") != expected_head:
        failures.append("merge PR head changed before authorization")
    if expected_body is not None and details.get("body") != expected_body:
        failures.append("merge PR body relationship changed before authorization")
    state = str(details.get("state", "")).upper()
    if state not in {"OPEN", "MERGED"}:
        failures.append("merge PR is not open")
    if state == "OPEN" and details.get("isDraft") is not False:
        failures.append("merge PR is still a draft")
    default_branch = gh_json(["repo", "view", repo, "--json", "defaultBranchRef,allowAutoMerge"])
    branch_ref = default_branch.get("defaultBranchRef") if isinstance(default_branch, dict) else None
    branch = branch_ref.get("name") if isinstance(branch_ref, dict) else None
    if not isinstance(branch, str) or not branch:
        raise SystemExit("repository default branch response was malformed")
    if branch != published_snapshot.branch:
        failures.append("repository default branch changed during published readiness")
    if details.get("baseRefName") != branch:
        failures.append("merge PR does not target the repository default branch")
    approval_requirement: int | None = None
    try:
        approval_requirement = merge_authorization_policy(repo, branch)
    except BaseException as error:
        failures.append(str(error))
    if state == "OPEN" and details.get("mergeable") != "MERGEABLE":
        failures.append("merge PR is not currently mergeable")
    if state == "MERGED":
        merge_commit = details.get("mergeCommit")
        if not isinstance(merge_commit, dict) or not isinstance(merge_commit.get("oid"), str):
            failures.append("merged PR did not expose an exact merge commit")
    references = details.get("closingIssuesReferences")
    if not isinstance(references, list) or len(references) > MAX_PULL_REQUEST_REFERENCES:
        failures.append("merge PR closing references exceeded the bounded limit")
        references = []
    owner_issue_number, owner_graph, owner_failures = resolve_pull_request_owner(
        repo, details, release_graphs
    )
    failures.extend(owner_failures)
    author = details.get("author")
    if not isinstance(author, dict) or not isinstance(author.get("login"), str):
        failures.append("merge PR author identity was malformed")
    elif author["login"].casefold() == "dependabot[bot]":
        failures.append(
            "Dependabot pull requests are not eligible for one-shot merge authorization"
        )
    review_decision = details.get("reviewDecision")
    if state == "OPEN" and approval_requirement is not None:
        if review_decision == "CHANGES_REQUESTED":
            failures.append("merge PR review decision is CHANGES_REQUESTED")
        elif not (review_decision is None or review_decision == "APPROVED"):
            failures.append(
                "merge PR review decision is not ready: "
                + (review_decision if isinstance(review_decision, str) else "malformed")
            )
    failures.extend(unresolved_review_failures(repo, number))
    base_sha = default_branch_head(repo, branch)
    if base_sha.casefold() != published_snapshot.sha.casefold():
        failures.append("default branch changed during published readiness")
    if expected_base_sha is not None and base_sha != expected_base_sha:
        failures.append("default branch changed during merge authorization")
    required = required_check_contexts(repo, branch)
    statuses, checks = check_run_collections(repo, expected_head)
    for context in required:
        if context == MERGE_AUTHORIZATION_STATUS_CONTEXT:
            continue
        matching = [
            check
            for check in checks
            if check.get("name") == context
            and isinstance(check.get("app"), dict)
            and check["app"].get("id") == GITHUB_ACTIONS_APP_ID
        ]
        matching.extend(
            status
            for status in statuses
            if status.get("context") == context
            and isinstance(status.get("creator"), dict)
            and status["creator"].get("login") == "github-actions[bot]"
        )
        if not matching or not any(
            item.get("conclusion") == "success" or item.get("state") == "success"
            for item in matching
        ):
            failures.append(f"required check {context!r} is not successful from GitHub Actions")
    failures.extend(release_graph_failures(repo, release_graphs, issue_map))
    if not (
        not references
        and owner_issue_number is not None
        and owner_graph is not None
        and owner_issue_number == owner_graph.release_issue
    ):
        for reference in references:
            try:
                failures.extend(
                    implementation_reference_failures(
                        repo, reference, root, issue_map, release_graphs
                    )
                )
            except BaseException as error:
                failures.append(str(error))
    return failures, {"branch": branch, "base_sha": base_sha, "details": details}


def graphql_mutation(query: str, variables: dict[str, object]) -> dict[str, object]:
    args = ["graphql", "-f", f"query={query}"]
    for key, value in variables.items():
        args.extend(["-F", f"{key}={value}"])
    payload = gh_api_json(args)
    if not isinstance(payload, dict) or payload.get("errors"):
        raise SystemExit("GitHub GraphQL mutation failed")
    return payload


def enable_auto_merge(repo: str, node_id: str, expected_head: str) -> None:
    payload = graphql_mutation(
        """
        mutation($pullRequestId:ID!, $expectedHeadOid:GitObjectID!) {
          enablePullRequestAutoMerge(input:{pullRequestId:$pullRequestId, mergeMethod:SQUASH, expectedHeadOid:$expectedHeadOid}) {
            pullRequest { autoMergeRequest { enabledAt } }
          }
        }
        """,
        {"pullRequestId": node_id, "expectedHeadOid": expected_head},
    )
    try:
        auto_merge_request = payload["data"]["enablePullRequestAutoMerge"]["pullRequest"][
            "autoMergeRequest"
        ]
    except (KeyError, TypeError):
        raise SystemExit("GitHub did not confirm enabling squash auto-merge")
    if not isinstance(auto_merge_request, dict):
        raise SystemExit("GitHub did not confirm enabling squash auto-merge")


def disable_auto_merge(repo: str, node_id: str) -> None:
    graphql_mutation(
        """
        mutation($pullRequestId:ID!) {
          disablePullRequestAutoMerge(input:{pullRequestId:$pullRequestId}) {
            pullRequest { number }
          }
        }
        """,
        {"pullRequestId": node_id},
    )


def wait_for_merged_pr(repo: str, number: int, expected_head: str, branch: str) -> bool:
    for _ in range(12):
        details = gh_json(
            [
                "pr",
                "view",
                str(number),
                "-R",
                repo,
                "--json",
                "state,headRefOid,baseRefName,mergeCommit",
            ]
        )
        if isinstance(details, dict) and details.get("state") == "MERGED":
            merge_commit = details.get("mergeCommit")
            return (
                details.get("headRefOid") == expected_head
                and details.get("baseRefName") == branch
                and isinstance(merge_commit, dict)
                and isinstance(merge_commit.get("oid"), str)
            )
        time.sleep(5)
    return False


def authorize_merge(
    repo: str,
    number: int,
    expected_head: str,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    request_id: str,
    actor: str,
    sender: str,
    published_snapshot: PublishedSnapshot | None = None,
) -> str:
    owner, _ = repo_parts(repo)
    if actor != owner or sender != owner:
        raise SystemExit("merge dispatch actor and sender must equal the repository owner")
    if published_snapshot is None:
        published_snapshot = require_published_snapshot(repo, root)
    initial = pull_request_readback(repo, number, expected_head_sha=expected_head)
    milestone_failures = pull_request_milestone_failures(repo, initial, release_graphs)
    if milestone_failures:
        raise SystemExit("merge authorization milestone preflight failed:\n" + "\n".join(milestone_failures))
    commit_status(
        repo,
        expected_head,
        "pending",
        "Validating one-shot merge authorization",
        MERGE_AUTHORIZATION_STATUS_CONTEXT,
    )
    auto_merge_enabled = False
    node_id: str | None = None
    try:
        failures, state = merge_readiness_failures(
            repo,
            number,
            expected_head,
            root,
            issue_map,
            release_graphs,
            published_snapshot=published_snapshot,
            expected_body=initial.get("body") if isinstance(initial.get("body"), str) else None,
        )
        details = state["details"]
        references = details.get("closingIssuesReferences") if isinstance(details, dict) else None
        current = pull_request_readback(
            repo,
            number,
            expected_head_sha=expected_head,
            expected_references=references if isinstance(references, list) else None,
            expected_body=initial.get("body") if isinstance(initial.get("body"), str) else None,
        )
        node_id = current.get("id")
        if str(details.get("state", "")).upper() == "MERGED" and not failures:
            final_current = pull_request_readback(
                repo,
                number,
                expected_head_sha=expected_head,
                expected_references=current["closingIssuesReferences"],
                expected_body=current.get("body") if isinstance(current.get("body"), str) else None,
            )
            final_milestone_failures = pull_request_milestone_failures(
                repo, final_current, release_graphs
            )
            if final_milestone_failures:
                raise SystemExit(
                    "merge authorization final milestone read-back failed:\n"
                    + "\n".join(final_milestone_failures)
                )
            commit_status(
                repo, expected_head, "success", "Merge already confirmed", MERGE_AUTHORIZATION_STATUS_CONTEXT
            )
            return "already-satisfied"
        if failures or not isinstance(node_id, str):
            raise SystemExit("merge authorization preflight failed:\n" + "\n".join(failures))
        enable_auto_merge(repo, node_id, expected_head)
        auto_merge_enabled = True
        final_failures, final_state = merge_readiness_failures(
            repo,
            number,
            expected_head,
            root,
            issue_map,
            release_graphs,
            expected_base_sha=state["base_sha"],
            published_snapshot=published_snapshot,
            expected_body=state["details"].get("body")
            if isinstance(state.get("details"), dict)
            and isinstance(state["details"].get("body"), str)
            else None,
        )
        if final_failures:
            raise SystemExit("merge authorization final re-read failed:\n" + "\n".join(final_failures))
        final_details = final_state["details"]
        final_references = (
            final_details.get("closingIssuesReferences")
            if isinstance(final_details, dict)
            else None
        )
        final_current = pull_request_readback(
            repo,
            number,
            expected_head_sha=expected_head,
            expected_references=final_references if isinstance(final_references, list) else None,
            expected_body=final_details.get("body")
            if isinstance(final_details, dict) and isinstance(final_details.get("body"), str)
            else None,
        )
        final_milestone_failures = pull_request_milestone_failures(
            repo, final_current, release_graphs
        )
        if final_milestone_failures:
            raise SystemExit(
                "merge authorization final milestone read-back failed:\n"
                + "\n".join(final_milestone_failures)
            )
        commit_status(
            repo,
            expected_head,
            "success",
            "Merge authorization passed; awaiting merge read-back",
            MERGE_AUTHORIZATION_STATUS_CONTEXT,
        )
        if not wait_for_merged_pr(repo, number, expected_head, final_state["branch"]):
            raise SystemExit("merge authorization did not receive an exact merged read-back")
        return "applied"
    except BaseException:
        if auto_merge_enabled and node_id is not None:
            try:
                pull_request_readback(repo, number, expected_head_sha=expected_head)
                disable_auto_merge(repo, node_id)
            except BaseException:
                pass
        try:
            live = pull_request_readback(repo, number)
            live_sha = live["headRefOid"]
            if isinstance(live_sha, str):
                commit_status(
                    repo,
                    live_sha,
                    "failure",
                    "Merge authorization failed; no merge was confirmed",
                    MERGE_AUTHORIZATION_STATUS_CONTEXT,
                )
        except BaseException:
            pass
        raise


def implementation_issue_failures(
    repo: str, issue: dict[str, object], graphs: dict[str, ReleaseGraph]
) -> list[str]:
    """Require every declared direct blocker of an issue to be closed."""

    number = positive_issue(issue.get("number"), "issue number")
    graph = graph_for_issue(graphs, number)
    if graph is None:
        return []
    failures: list[str] = []
    for blocker in graph.blocked_by[number]:
        try:
            blocker_issue = issue_payload(repo, blocker)
        except BaseException as error:
            failures.append(_live_graph_failure(f"reading blocker #{blocker}", error))
            continue
        state = str(blocker_issue.get("state", "")).upper()
        if state != "CLOSED":
            failures.append(
                f"#{number} is not implementation/merge-ready: direct blocker #{blocker} is "
                f"{state or 'UNKNOWN'}, not CLOSED"
            )
    return failures


def invalidate_issue_readiness(
    repo: str, issue: int, release_graphs: dict[str, ReleaseGraph] | None = None
) -> None:
    """Invalidate bounded open PR readiness statuses linked to one affected issue."""

    pull_requests = gh_json(
        [
            "pr",
            "list",
            "-R",
            repo,
            "--state",
            "open",
            "--search",
            f"#{issue} in:body",
            "--limit",
            str(MAX_REPAIR_DEPENDENTS + 1),
            "--json",
            "number,headRefOid,closingIssuesReferences",
        ]
    )
    if not isinstance(pull_requests, list):
        raise SystemExit("GitHub affected PR response was malformed")
    if len(pull_requests) > MAX_REPAIR_DEPENDENTS:
        raise SystemExit("GitHub affected PR response exceeded the bounded repair limit")
    failures: list[str] = []
    for pull_request in pull_requests:
        if not isinstance(pull_request, dict):
            failures.append("GitHub affected PR response contained a non-object")
            continue
        try:
            number = positive_issue(
                pull_request.get("number"), "affected pull request number"
            )
            head = pull_request.get("headRefOid")
            references = pull_request.get("closingIssuesReferences")
            if re.fullmatch(r"[0-9a-fA-F]{40}", head or "") is None:
                raise SystemExit(f"affected PR #{number} did not expose an exact head SHA")
            if not isinstance(references, list) or len(references) > MAX_PULL_REQUEST_REFERENCES:
                raise SystemExit(
                    f"affected PR #{number} references were malformed or unbounded"
                )
            matches_issue = False
            for reference in references:
                referenced_issue = native_relation_issue_number(
                    repo, reference, "affected PR closing issue reference"
                )
                matches_issue = matches_issue or referenced_issue == issue
            if not matches_issue:
                continue
            pull_request_readback(
                repo,
                number,
                expected_head_sha=head,
                expected_references=references,
            )
        except BaseException as error:
            failures.append(f"PR #{pull_request.get('number', '?')} target validation failed: {error}")
            continue

        try:
            revoke_merge_authorization_for_pr(
                repo,
                number,
                expected_head_sha=head,
                expected_references=references,
                release_graphs=release_graphs,
            )
        except BaseException as error:
            failures.append(f"PR #{number} merge authorization invalidation failed: {error}")
        try:
            pull_request_readback(
                repo,
                number,
                expected_head_sha=head,
                expected_references=references,
            )
            commit_status(
                repo,
                head,
                "failure",
                "Implementation readiness invalidated by an open declared blocker",
                IMPLEMENTATION_STATUS_CONTEXT,
            )
        except BaseException as error:
            failures.append(f"PR #{number} implementation invalidation failed: {error}")
    if failures:
        raise SystemExit("issue readiness invalidation failed:\n" + "\n".join(failures))


def repair_reopened_blocker(
    repo: str, blocker: int, graphs: dict[str, ReleaseGraph]
) -> None:
    """Repair graph-bounded closed dependents after a blocker becomes open."""

    queue = [blocker]
    seen = {blocker}
    while queue:
        current_blocker = queue.pop(0)
        for dependent in reverse_declared_dependents(graphs, current_blocker):
            if dependent in seen:
                continue
            seen.add(dependent)
            if len(seen) > MAX_REPAIR_DEPENDENTS:
                raise SystemExit("reverse release-graph repair exceeded the bounded limit")
            dependent_issue = issue_payload(repo, dependent)
            state = str(dependent_issue.get("state", "")).upper()
            if state == "CLOSED":
                run(["gh", "issue", "reopen", str(dependent), "--repo", repo])
            elif state != "OPEN":
                raise SystemExit(
                    f"dependent #{dependent} has an invalid state {state or 'UNKNOWN'}"
                )
            invalidate_issue_readiness(repo, dependent, graphs)
            queue.append(dependent)


def enforce_closed_issue_blockers(
    repo: str, issue_number: int, graphs: dict[str, ReleaseGraph]
) -> None:
    """Repair a closed declared issue when a direct blocker is still open."""

    issue = issue_payload(repo, positive_issue(issue_number, "issue number"))
    state = str(issue.get("state", "")).upper()
    if state == "OPEN":
        repair_reopened_blocker(repo, issue_number, graphs)
        return
    if state != "CLOSED":
        return
    failures = implementation_issue_failures(repo, issue, graphs)
    if not failures:
        return
    run(["gh", "issue", "reopen", str(issue_number), "--repo", repo])
    invalidate_issue_readiness(repo, issue_number, graphs)
    repair_reopened_blocker(repo, issue_number, graphs)
    raise SystemExit(
        "closed issue was reopened because blocker enforcement failed:\n"
        + "\n".join(f"- {failure}" for failure in failures)
    )


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
        tasks = issue_checklist_tasks(issue)
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
- [ ] Keep the contract synchronized. (OpenSpec tasks: 2.1)
## OpenSpec Tasks
## 1. Review
- [x] 1.1 Anchored task
## 2. Implementation
- [ ] 2.1 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
"""
    self_test_root = Path(__file__).resolve().parents[2]

    def contract_failures(
        issue: dict[str, object], tasks: list[tuple[bool, str]]
    ) -> list[str]:
        return issue_contract_failures(issue, tasks, "owner/repo", self_test_root)

    assert contract_failures({"state": "OPEN", "body": issue_contract}, expected) == []
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
        "## OpenSpec Tasks",
        "+ [ ] Unbound mitigation\n## OpenSpec Tasks",
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
    completed_contract = issue_contract.replace(
        "- [ ] Keep the contract synchronized.",
        "- [x] Keep the contract synchronized.",
    ).replace(
        "- [ ] 2.1 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.",
        "- [x] 2.1 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.",
    )
    assert contract_failures(
        {"state": "OPEN", "body": completed_contract},
        [
            (True, "1.1 Anchored task"),
            (
                True,
                "2.1 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.",
            ),
        ],
    ) == []
    missing_scope = issue_contract.replace("## Release Scope", "## Delivery")
    assert any(
        "'release scope'" in failure
        for failure in contract_failures({"state": "OPEN", "body": missing_scope}, expected)
    )
    unknown_task = issue_contract.replace(
        "(OpenSpec tasks: 2.1)", "(OpenSpec tasks: 9.9)"
    )
    assert any(
        "unknown or foreign" in failure
        for failure in contract_failures({"state": "OPEN", "body": unknown_task}, expected)
    )
    assert contract_failures({"state": "CLOSED", "body": ""}, expected) == []
    wrong_final = [expected[0], (False, "2.1 Finish ordinary tests.")]
    assert any(
        "final OpenSpec task must be the architecture acceptance task" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": issue_contract}, wrong_final
        )
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
    assert mapped_issue_numbers({"one": (Owner(1),), "two": (Owner(2),)}) == {
        1,
        2,
    }

    def graph_failure(payload: object, expected: str) -> None:
        try:
            parse_release_graphs(
                payload,
                Path("issue-map.json"),
                {"a": (Owner(10),), "b": (Owner(11),), "release": (Owner(12),)},
            )
        except SystemExit as error:
            assert expected in str(error), str(error)
        else:
            raise AssertionError(f"release graph accepted invalid case: {expected}")

    graph_owners = {
        "a": (Owner(10),),
        "b": (Owner(11),),
        "release": (Owner(12),),
    }
    valid_graph_payload = {
        "schema_version": 2,
        "changes": {},
        "release_graphs": {
            "v1.2.3-00": {
                "release_issue": 12,
                "issues": {"10": [11], "11": [], "12": [10, 11]},
            }
        },
    }
    valid_graphs = parse_release_graphs(valid_graph_payload, Path("issue-map.json"), graph_owners)
    assert valid_graphs["v1.2.3-00"].blocked_by[12] == (10, 11)
    declared_issue = {"number": 10, "milestone": {"title": "v1.2.3-00"}}
    assert target_graph_failures(declared_issue, valid_graphs) == (
        valid_graphs["v1.2.3-00"],
        [],
    )
    wrong_milestone_issue = {"number": 10, "milestone": {"title": "v9.9.9-00"}}
    wrong_milestone_graph, wrong_milestone_failures = target_graph_failures(
        wrong_milestone_issue, valid_graphs
    )
    assert wrong_milestone_graph == valid_graphs["v1.2.3-00"]
    assert any("declared in release graph v1.2.3-00" in failure for failure in wrong_milestone_failures)
    unset_milestone_graph, unset_milestone_failures = target_graph_failures(
        {"number": 10}, valid_graphs
    )
    assert unset_milestone_graph == valid_graphs["v1.2.3-00"]
    assert any("live milestone is unset" in failure for failure in unset_milestone_failures)
    assert target_graph_failures({"number": 99}, valid_graphs) == (None, [])
    graph_failure({"schema_version": 2, "changes": {}, "release_graphs": None}, "must be an object")
    graph_failure({"schema_version": 2, "changes": {}, "release_graphs": []}, "must be an object")
    graph_failure({"schema_version": 2, "changes": {}, "release_graphs": "invalid"}, "must be an object")
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": valid_graph_payload["release_graphs"]["v1.2.3-00"],
                "v2.3.4-00": {
                    "release_issue": 12,
                    "issues": {"12": []},
                },
            },
        },
        "appears in release graphs",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"010": [11], "11": [], "12": [10, 11]},
                }
            },
        },
        "must be issue numbers",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"١٠": [11], "11": [], "12": [10, 11]},
                }
            },
        },
        "must be issue numbers",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"10": [11], "11": [10], "12": [10, 11]},
                }
            },
        },
        "dependency cycle",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"10": [13], "11": [], "12": [10, 11]},
                }
            },
        },
        "unknown graph issue",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"10": [], "11": [], "12": [10, 11], "13": []},
                }
            },
        },
        "has no local OpenSpec mapping",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"10": [11, 11], "11": [], "12": [10, 11]},
                }
            },
        },
        "duplicate issue numbers",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"10": [10], "11": [], "12": [10, 11]},
                }
            },
        },
        "cannot block itself",
    )
    graph_failure(
        {
            **valid_graph_payload,
            "release_graphs": {
                "v1.2.3-00": {
                    "release_issue": 12,
                    "issues": {"10": [11], "11": [], "12": [10]},
                }
            },
        },
        "must be directly blocked by every other",
    )
    assert parse_release_graphs(
        {"schema_version": 2, "changes": {"legacy": 1}},
        Path("issue-map.json"),
        {"legacy": (Owner(1),)},
    ) == {}

    saved_milestone_issues = globals()["milestone_issues"]
    saved_native_blocked_by = globals()["native_blocked_by"]
    saved_native_sub_issues = globals()["native_sub_issues"]
    saved_native_parent_issue = globals()["native_parent_issue"]
    saved_issue_payload = globals()["issue_payload"]
    saved_release_graph_failures = globals()["release_graph_failures"]
    saved_gh_json = globals()["gh_json"]
    saved_commit_status = globals()["commit_status"]
    saved_disable_auto_merge = globals()["disable_auto_merge"]
    saved_published_snapshot = globals()["require_published_snapshot"]
    saved_gh_api_json = globals()["gh_api_json"]
    saved_run = globals()["run"]
    saved_subprocess_run = subprocess.run
    try:
        api_args: list[list[str]] = []
        local_identity = {"repository_url": "https://api.github.com/repos/owner/repo"}

        def fake_gh_api_json(args: list[str]) -> object:
            api_args.append(args)
            joined = " ".join(args)
            if "/milestones" in joined:
                return [{"title": "v1.2.3-00", "number": 7}]
            if "/dependencies/blocked_by" in joined:
                return [{**local_identity, "number": 11}]
            if "milestone=7" in joined:
                return [{"number": 10}, {"number": 11}]
            return [
                {**local_identity, "number": 10},
                {**local_identity, "number": 11},
            ]

        globals()["gh_api_json"] = fake_gh_api_json
        assert native_blocked_by("owner/repo", 10) == {11}
        assert native_sub_issues("owner/repo", 12) == {10, 11}

        class FakeProcess:
            def __init__(self, payload: object) -> None:
                self.returncode = 0
                self.stdout = "HTTP/2.0 200 OK\r\n\r\n" + json.dumps(payload)
                self.stderr = ""

        subprocess.run = lambda *args, **kwargs: FakeProcess(
            {**local_identity, "number": 12}
        )
        assert native_parent_issue("owner/repo", 10) == 12

        native_mutation_state = {"blocked_by": set(), "sub_issue": set()}
        native_mutation_calls: list[list[str]] = []

        def fake_native_api(args: list[str]) -> object:
            path = args[0]
            if path.endswith("/dependencies/blocked_by"):
                return (
                    [{**local_identity, "number": 11}]
                    if 11 in native_mutation_state["blocked_by"]
                    else []
                )
            if path.endswith("/sub_issues"):
                return (
                    [{**local_identity, "number": 11}]
                    if 11 in native_mutation_state["sub_issue"]
                    else []
                )
            if path.endswith("/issues/11"):
                return {**local_identity, "number": 11, "id": 1100}
            raise AssertionError(f"unexpected native mutation API call: {args}")

        def fake_native_run(args: list[str]) -> str:
            native_mutation_calls.append(args)
            path = args[2]
            method = args[4]
            kind = "blocked_by" if "/dependencies/blocked_by" in path else "sub_issue"
            if method == "POST":
                native_mutation_state[kind].add(11)
            else:
                native_mutation_state[kind].discard(11)
            return ""

        globals()["gh_api_json"] = fake_native_api
        globals()["run"] = fake_native_run
        globals()["native_parent_issue"] = lambda repo, issue: None
        assert mutate_native_relationship("owner/repo", 10, 11, "blocked_by", "add")
        assert native_mutation_calls[-1][2].endswith(
            "/issues/10/dependencies/blocked_by"
        )
        assert native_mutation_calls[-1][4] == "POST"
        assert native_mutation_calls[-1][-2:] == ["-F", "issue_id=1100"]
        call_count = len(native_mutation_calls)
        assert not mutate_native_relationship("owner/repo", 10, 11, "blocked_by", "add")
        assert len(native_mutation_calls) == call_count
        assert mutate_native_relationship("owner/repo", 10, 11, "blocked_by", "remove")
        assert native_mutation_calls[-1][2].endswith(
            "/issues/10/dependencies/blocked_by/1100"
        )
        assert native_mutation_calls[-1][4] == "DELETE"
        assert "-F" not in native_mutation_calls[-1]
        assert mutate_native_relationship("owner/repo", 10, 11, "sub_issue", "add")
        assert native_mutation_calls[-1][2].endswith("/issues/10/sub_issues")
        assert native_mutation_calls[-1][4] == "POST"
        assert native_mutation_calls[-1][-2:] == ["-F", "sub_issue_id=1100"]
        assert mutate_native_relationship("owner/repo", 10, 11, "sub_issue", "remove")
        assert native_mutation_calls[-1][2].endswith("/issues/10/sub_issue")
        assert native_mutation_calls[-1][4] == "DELETE"
        assert native_mutation_calls[-1][-2:] == ["-F", "sub_issue_id=1100"]
        globals()["native_parent_issue"] = saved_native_parent_issue

        globals()["release_graph_failures"] = lambda *args, **kwargs: []
        native_mutation_calls.clear()
        native_mutation_state["blocked_by"] = set()
        for relation_kind, issue, related in (
            ("blocked_by", 99, 11),
            ("sub_issue", 10, 11),
        ):
            try:
                mutate_native_relationship_and_revalidate(
                    "owner/repo",
                    issue,
                    related,
                    relation_kind,
                    "remove",
                    graph_owners,
                    valid_graphs,
                    f"rel-unowned-{relation_kind}",
                )
            except SystemExit as error:
                assert "exactly one release graph" in str(error)
                assert native_mutation_calls == []
            else:
                raise AssertionError(f"unowned {relation_kind} source was admitted")
        ambiguous_graphs = {
            **valid_graphs,
            "v9.9.9-00": ReleaseGraph(
                "v9.9.9-00", 12, {10: (11,), 11: (), 12: (10, 11)}
            ),
        }
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                12,
                10,
                "sub_issue",
                "add",
                graph_owners,
                ambiguous_graphs,
                "rel-ambiguous-sub-issue",
            )
        except SystemExit as error:
            assert "exactly one release graph" in str(error)
            assert native_mutation_calls == []
        else:
            raise AssertionError("multiply-owned sub-issue source was admitted")
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                10,
                99,
                "blocked_by",
                "add",
                graph_owners,
                valid_graphs,
                "rel-invalid-add",
            )
        except SystemExit as error:
            assert "is not declared" in str(error)
            assert native_mutation_calls == []
        else:
            raise AssertionError("undeclared native add was admitted")
        native_mutation_state["blocked_by"] = {11}
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                10,
                11,
                "blocked_by",
                "remove",
                graph_owners,
                valid_graphs,
                "rel-invalid-remove",
            )
        except SystemExit as error:
            assert "erase a declared relation" in str(error)
            assert native_mutation_calls == []
        else:
            raise AssertionError("declared native removal was admitted")
        native_mutation_state["blocked_by"] = set()
        native_mutation_state["sub_issue"] = set()
        globals()["release_graph_failures"] = lambda *args, **kwargs: [
            "unrelated declared drift"
        ]
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                10,
                11,
                "blocked_by",
                "add",
                graph_owners,
                valid_graphs,
                "rel-valid-with-drift",
            )
        except SystemExit as error:
            assert "reconciliation failed" in str(error)
            assert native_mutation_calls[-1][4] == "POST"
        else:
            raise AssertionError("reconciliation drift was masked after valid mutation")
        globals()["release_graph_failures"] = saved_release_graph_failures

        relationship_snapshot = PublishedSnapshot("main", "b" * 40)
        relationship_snapshot_mode = "stable"
        relationship_snapshot_calls = 0

        def fake_relationship_snapshot(repo: str, root: Path) -> PublishedSnapshot:
            nonlocal relationship_snapshot_calls
            relationship_snapshot_calls += 1
            if relationship_snapshot_mode == "before-preflight":
                return PublishedSnapshot("main", "d" * 40)
            if relationship_snapshot_mode == "before-write" and relationship_snapshot_calls >= 2:
                return PublishedSnapshot("main", "d" * 40)
            if relationship_snapshot_mode == "after-write" and relationship_snapshot_calls >= 3:
                return PublishedSnapshot("main", "d" * 40)
            return relationship_snapshot

        globals()["require_published_snapshot"] = fake_relationship_snapshot
        native_mutation_state["blocked_by"] = set()
        native_mutation_state["sub_issue"] = set()
        native_mutation_calls.clear()
        relationship_snapshot_mode = "before-preflight"
        relationship_snapshot_calls = 0
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                10,
                11,
                "blocked_by",
                "add",
                graph_owners,
                valid_graphs,
                "rel-snapshot-before-preflight",
                root=self_test_root,
                published_snapshot=relationship_snapshot,
            )
        except SystemExit as error:
            assert "before native relationship preflight" in str(error)
        else:
            raise AssertionError("moved snapshot before preflight was accepted")
        assert native_mutation_calls == []

        relationship_snapshot_mode = "before-write"
        relationship_snapshot_calls = 0
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                10,
                11,
                "blocked_by",
                "add",
                graph_owners,
                valid_graphs,
                "rel-snapshot-before-write",
                root=self_test_root,
                published_snapshot=relationship_snapshot,
            )
        except SystemExit as error:
            assert "before native relationship mutation" in str(error)
        else:
            raise AssertionError("moved snapshot before write was accepted")
        assert native_mutation_calls == []

        relationship_snapshot_mode = "after-write"
        relationship_snapshot_calls = 0
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                10,
                11,
                "blocked_by",
                "add",
                graph_owners,
                valid_graphs,
                "rel-snapshot-after-write",
                root=self_test_root,
                published_snapshot=relationship_snapshot,
            )
        except SystemExit as error:
            assert "remote read-back" in str(error)
            assert "no success outcome" in str(error)
        else:
            raise AssertionError("moved snapshot after write was reported as success")
        assert len(native_mutation_calls) == 1
        assert native_mutation_calls[0][4] == "POST"
        assert native_mutation_state["blocked_by"] == {11}

        foreign_identity = {
            "repository_url": "https://api.github.com/repos/foreign/repo",
            "number": 11,
        }
        rest_full_name_identity = {
            "repository": {"full_name": "owner/repo"},
            "number": 11,
        }
        rest_url_identity = {
            "repository_url": "https://api.github.com/repos/owner/repo",
            "number": 11,
        }
        nested_gh_identity = {
            "repository": {"name": "repo", "owner": {"login": "owner"}},
            "number": 11,
        }
        assert native_relation_issue_number(
            "owner/repo", rest_full_name_identity, "native relation"
        ) == 11
        assert native_relation_issue_number(
            "owner/repo", rest_url_identity, "native relation"
        ) == 11
        assert native_relation_issue_number(
            "owner/repo", nested_gh_identity, "native relation"
        ) == 11
        try:
            native_relation_issue_number(
                "owner/repo",
                {
                    "repository_url": "https://api.github.com/repos/owner/repo",
                    "repository": {
                        "name": "repo",
                        "owner": {"login": "foreign"},
                    },
                    "number": 11,
                },
                "native relation",
            )
        except SystemExit as error:
            assert "exact owner/repo repository identity" in str(error)
        else:
            raise AssertionError("conflicting REST and gh repository identities were accepted")
        for label in (
            "blocked-by issue number",
            "sub-issue number",
            "parent issue number",
        ):
            try:
                native_relation_issue_number("owner/repo", foreign_identity, label)
            except SystemExit as error:
                assert "exact owner/repo repository identity" in str(error)
            else:
                raise AssertionError(f"foreign {label} was accepted")
        for malformed in (
            {"number": 11},
            {"number": 11, "repository_url": 12},
            {"number": 11, "repository": {"full_name": 12}},
            {"number": 11, "repository": {"name": "repo"}},
            {"number": 11, "repository": {"owner": {"login": "owner"}}},
            {"number": 11, "repository": {"name": "repo", "owner": {"login": 12}}},
        ):
            try:
                native_relation_issue_number("owner/repo", malformed, "native relation")
            except SystemExit as error:
                assert "repository identity" in str(error)
            else:
                raise AssertionError("missing or malformed native identity was accepted")

        globals()["gh_api_json"] = lambda args: [foreign_identity]
        for relation, label in (
            (native_blocked_by, "blocked-by issue number"),
            (native_sub_issues, "sub-issue number"),
        ):
            try:
                relation("owner/repo", 10)
            except SystemExit as error:
                assert label in str(error)
            else:
                raise AssertionError(f"foreign {label} relation was accepted")
        subprocess.run = lambda *args, **kwargs: FakeProcess(foreign_identity)
        try:
            native_parent_issue("owner/repo", 10)
        except SystemExit as error:
            assert "parent issue number" in str(error)
        else:
            raise AssertionError("foreign parent relation was accepted")
        globals()["gh_api_json"] = fake_gh_api_json
        assert milestone_number("owner/repo", "v1.2.3-00") == 7
        assert milestone_issues("owner/repo", "v1.2.3-00") == [
            {"number": 10},
            {"number": 11},
        ]
        for matching in (
            [args for args in api_args if "/milestones" in " ".join(args)],
            [args for args in api_args if "milestone=7" in args],
        ):
            assert matching and any(
                GITHUB_API_VERSION in argument for argument in matching[0]
            )
        globals()["milestone_issues"] = lambda repo, milestone: [
            {"number": 10},
            {"number": 11},
            {"number": 12},
        ]
        globals()["native_sub_issues"] = lambda repo, issue: {10, 11}
        globals()["native_parent_issue"] = lambda repo, issue: (
            None if issue == 12 else 12
        )
        globals()["native_blocked_by"] = lambda repo, issue: {
            10: {11},
            11: set(),
            12: {10, 11},
        }[issue]
        assert release_graph_failures("owner/repo", valid_graphs, graph_owners) == []
        globals()["milestone_issues"] = lambda repo, milestone: [
            {"number": 10},
            {"number": 11},
            {"number": 12},
            {"number": 99},
        ]
        membership_failures = release_graph_failures("owner/repo", valid_graphs, graph_owners)
        assert any("missing from release graph" in failure for failure in membership_failures)
        globals()["milestone_issues"] = lambda repo, milestone: [
            {"number": 10},
            {"number": 11},
            {"number": 12},
        ]
        globals()["native_blocked_by"] = lambda repo, issue: {
            10: set(),
            11: {99},
            12: {10, 11},
        }[issue]
        live_failures = release_graph_failures("owner/repo", valid_graphs, graph_owners)
        assert any("missing native blocked-by" in failure for failure in live_failures)
        assert any("extra native blocked-by" in failure for failure in live_failures)
        globals()["native_sub_issues"] = lambda repo, issue: {10}
        hierarchy_failures = release_graph_failures("owner/repo", valid_graphs, graph_owners)
        assert any("missing native sub-issue" in failure for failure in hierarchy_failures)
        globals()["native_parent_issue"] = lambda repo, issue: 99 if issue == 10 else None
        hierarchy_failures = release_graph_failures("owner/repo", valid_graphs, graph_owners)
        assert any("native parent #99" in failure for failure in hierarchy_failures)
        globals()["native_parent_issue"] = lambda repo, issue: 99 if issue == 12 else 12
        hierarchy_failures = release_graph_failures("owner/repo", valid_graphs, graph_owners)
        assert any("must not have native parent #99" in failure for failure in hierarchy_failures)
        globals()["issue_payload"] = lambda repo, issue: {
            "number": issue,
            "state": {11: "OPEN", 12: "CLOSED"}.get(issue, "CLOSED"),
        }
        assert any(
            "blocker #11" in failure
            for failure in implementation_issue_failures(
                "owner/repo", {"number": 10}, valid_graphs
            )
        )
        globals()["issue_payload"] = lambda repo, issue: {
            "number": issue,
            "state": "CLOSED",
        }
        assert implementation_issue_failures(
            "owner/repo", {"number": 10}, valid_graphs
        ) == []
        repair_commands: list[list[str]] = []
        globals()["issue_payload"] = lambda repo, issue: {
            "number": issue,
            "state": "OPEN" if issue == 10 else "CLOSED",
        }
        globals()["run"] = lambda args: repair_commands.append(args) or ""
        globals()["gh_json"] = lambda args: []
        try:
            enforce_closed_issue_blockers("owner/repo", 12, valid_graphs)
        except SystemExit as error:
            assert "closed issue was reopened" in str(error)
            assert repair_commands == [["gh", "issue", "reopen", "12", "--repo", "owner/repo"]]
        else:
            raise AssertionError("closed issue with an open blocker was not repaired")
        repair_commands.clear()
        globals()["issue_payload"] = lambda repo, issue: {
            "number": issue,
            "state": {10: "CLOSED", 11: "OPEN", 12: "CLOSED"}.get(issue, "CLOSED"),
        }
        enforce_closed_issue_blockers("owner/repo", 11, valid_graphs)
        assert repair_commands == [
            ["gh", "issue", "reopen", "10", "--repo", "owner/repo"],
            ["gh", "issue", "reopen", "12", "--repo", "owner/repo"],
        ]
        affected_statuses: list[tuple[int, str]] = []
        disable_calls: list[str] = []
        disable_failures: set[str] = set()
        status_failures: set[tuple[int, str]] = set()
        invalidation_mode = "normal"

        def fake_invalidation_gh_json(args: list[str]) -> object:
            if args[:2] == ["pr", "list"]:
                if invalidation_mode == "foreign-same-number":
                    references = [{
                        "repository_url": "https://api.github.com/repos/foreign/repo",
                        "number": 10,
                    }]
                else:
                    references = [{**local_identity, "number": 10}]
                return [
                    {
                        "number": number,
                        "headRefOid": ("a" if number == 494 else "c") * 40,
                        "closingIssuesReferences": references,
                    }
                    for number in (494, 495)
                ]
            if args[:2] == ["pr", "view"]:
                number = int(args[2])
                return {
                    "number": number,
                    "headRefOid": "b" * 40
                    if invalidation_mode == "raced-head" and number == 494
                    else ("a" if number == 494 else "c") * 40,
                    "closingIssuesReferences": [{**local_identity, "number": 10}],
                    "id": f"PR_{number}",
                    "autoMergeRequest": {"id": f"AUTO_{number}"}
                    if number == 494
                    else None,
                    "milestone": {"title": "v1.2.3-00"},
                    "body": "",
                }
            if args[:2] == ["issue", "view"]:
                return {"number": int(args[2]), "milestone": {"title": "v1.2.3-00"}}
            raise AssertionError(f"unexpected invalidation gh_json call: {args}")

        def fake_invalidation_status(
            repo: str,
            sha: str,
            state: str,
            description: str,
            context: str = IMPLEMENTATION_STATUS_CONTEXT,
        ) -> None:
            number = 494 if sha == "a" * 40 else 495
            affected_statuses.append((number, context))
            if (number, context) in status_failures:
                raise RuntimeError(f"simulated {number} {context} status failure")

        globals()["gh_json"] = fake_invalidation_gh_json
        globals()["issue_payload"] = lambda repo, issue: {
            "number": issue,
            "milestone": {"title": "v1.2.3-00"},
        }
        globals()["commit_status"] = fake_invalidation_status

        def fake_disable_auto_merge(repo: str, node_id: str) -> None:
            disable_calls.append(node_id)
            if node_id in disable_failures:
                raise RuntimeError(f"simulated auto-merge disable failure for {node_id}")

        globals()["disable_auto_merge"] = fake_disable_auto_merge
        invalidate_issue_readiness("owner/repo", 10)
        assert affected_statuses == [
            (494, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (494, IMPLEMENTATION_STATUS_CONTEXT),
            (495, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (495, IMPLEMENTATION_STATUS_CONTEXT),
        ]
        assert disable_calls == ["PR_494"]
        affected_statuses.clear()
        disable_failures = {"PR_494"}
        try:
            invalidate_issue_readiness("owner/repo", 10)
        except SystemExit as error:
            assert "merge authorization invalidation failed" in str(error)
        else:
            raise AssertionError("auto-merge disable failure was masked")
        assert affected_statuses == [
            (494, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (494, IMPLEMENTATION_STATUS_CONTEXT),
            (495, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (495, IMPLEMENTATION_STATUS_CONTEXT),
        ]
        disable_failures.clear()
        affected_statuses.clear()
        status_failures = {(494, MERGE_AUTHORIZATION_STATUS_CONTEXT)}
        try:
            invalidate_issue_readiness("owner/repo", 10)
        except SystemExit as error:
            assert "issue readiness invalidation failed" in str(error)
        else:
            raise AssertionError("merge invalidation failure was masked")
        assert affected_statuses == [
            (494, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (494, IMPLEMENTATION_STATUS_CONTEXT),
            (495, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (495, IMPLEMENTATION_STATUS_CONTEXT),
        ]
        affected_statuses.clear()
        status_failures = {(494, IMPLEMENTATION_STATUS_CONTEXT)}
        try:
            invalidate_issue_readiness("owner/repo", 10)
        except SystemExit as error:
            assert "issue readiness invalidation failed" in str(error)
        else:
            raise AssertionError("implementation invalidation failure was masked")
        assert affected_statuses == [
            (494, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (494, IMPLEMENTATION_STATUS_CONTEXT),
            (495, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (495, IMPLEMENTATION_STATUS_CONTEXT),
        ]
        affected_statuses.clear()
        status_failures = {
            (494, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (494, IMPLEMENTATION_STATUS_CONTEXT),
            (495, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (495, IMPLEMENTATION_STATUS_CONTEXT),
        }
        try:
            invalidate_issue_readiness("owner/repo", 10)
        except SystemExit as error:
            assert "issue readiness invalidation failed" in str(error)
        else:
            raise AssertionError("all invalidation status failures were masked")
        affected_statuses.clear()
        status_failures.clear()
        invalidation_mode = "foreign-same-number"
        try:
            invalidate_issue_readiness("owner/repo", 10)
        except SystemExit as error:
            assert "exact owner/repo repository identity" in str(error)
        else:
            raise AssertionError("foreign same-number closing reference was accepted")
        assert affected_statuses == []
        invalidation_mode = "raced-head"
        try:
            invalidate_issue_readiness("owner/repo", 10)
        except SystemExit as error:
            assert "target validation failed" in str(error)
        else:
            raise AssertionError("raced PR head was accepted for invalidation")
        assert affected_statuses == [
            (495, MERGE_AUTHORIZATION_STATUS_CONTEXT),
            (495, IMPLEMENTATION_STATUS_CONTEXT),
        ]
    finally:
        globals()["milestone_issues"] = saved_milestone_issues
        globals()["native_blocked_by"] = saved_native_blocked_by
        globals()["native_sub_issues"] = saved_native_sub_issues
        globals()["native_parent_issue"] = saved_native_parent_issue
        globals()["issue_payload"] = saved_issue_payload
        globals()["gh_json"] = saved_gh_json
        globals()["commit_status"] = saved_commit_status
        globals()["disable_auto_merge"] = saved_disable_auto_merge
        globals()["require_published_snapshot"] = saved_published_snapshot
        globals()["gh_api_json"] = saved_gh_api_json
        globals()["run"] = saved_run
        globals()["release_graph_failures"] = saved_release_graph_failures
        subprocess.run = saved_subprocess_run

    entry_snapshot = PublishedSnapshot("main", "e" * 40)
    entry_capture: dict[str, object] = {}
    saved_entry_argv = sys.argv[:]
    saved_entry_load_issue_map = globals()["load_issue_map"]
    saved_entry_load_release_graphs = globals()["load_release_graphs"]
    saved_entry_mutation = globals()["mutate_native_relationship_and_revalidate"]

    def fake_entry_snapshot(repo: str, root: Path) -> PublishedSnapshot:
        entry_capture["repo"] = repo
        entry_capture["root"] = root
        return entry_snapshot

    def fake_entry_mutation(*args: object, **kwargs: object) -> None:
        entry_capture["snapshot"] = kwargs.get("published_snapshot")
        entry_capture["mutation_root"] = kwargs.get("root")

    globals()["require_published_snapshot"] = fake_entry_snapshot
    globals()["load_issue_map"] = lambda path: {}
    globals()["load_release_graphs"] = lambda path, issue_map: {}
    globals()["mutate_native_relationship_and_revalidate"] = fake_entry_mutation
    sys.argv = [
        "issue-checklists.py",
        "--repo",
        "owner/repo",
        "--root",
        str(self_test_root),
        "--issue-map",
        str(self_test_root / "openspec/issue-map.json"),
        "--mutate-native-relationship",
        "--native-relationship-kind",
        "blocked_by",
        "--native-relationship-operation",
        "add",
        "--native-relationship-issue",
        "10",
        "--native-related-issue",
        "11",
        "--request-id",
        "rel-entry-01",
    ]
    try:
        main()
    finally:
        sys.argv = saved_entry_argv
        globals()["require_published_snapshot"] = saved_published_snapshot
        globals()["load_issue_map"] = saved_entry_load_issue_map
        globals()["load_release_graphs"] = saved_entry_load_release_graphs
        globals()["mutate_native_relationship_and_revalidate"] = saved_entry_mutation
    assert entry_capture["repo"] == "owner/repo"
    assert entry_capture["root"] == self_test_root
    assert entry_capture["mutation_root"] == self_test_root
    assert entry_capture["snapshot"] is entry_snapshot

    assert milestone_issue_failures(
        "v1.0.0-00",
        [{"number": 1, "state": "closed"}, {"number": 3, "state": "open"}],
        {1, 2},
    ) == [
        "#3 in milestone v1.0.0-00 has no local OpenSpec mapping",
        "#3 in milestone v1.0.0-00 is OPEN, not CLOSED",
    ]
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
        (change / "tasks.md").write_text(
            "## 1. Contract\n- [x] 1.1 Specify the contract.\n"
            "## 2. Acceptance\n- [ ] 2.1 Review the final implementation against the "
            "architecture diagrams, update the diagrams or implementation until they "
            "agree, or reconfirm the reasoned N/A.\n",
            encoding="utf-8",
        )
        (readiness_root / "docs").mkdir()
        (readiness_root / "docs" / "candidate-only.md").write_text(
            "## Candidate Heading\n\nCandidate-only presentation.\n",
            encoding="utf-8",
        )
        assert openspec_readiness_failures(readiness_root, "ready-change") == []
        assert planned_issue_failures(
            ready_issue, {"ready-change": (Owner(448),)}, readiness_root
        ) == []
        try:
            require_published_snapshot("owner/repo", readiness_root)
        except SystemExit as error:
            assert "Git inspection failed" in str(error)
        else:
            raise AssertionError("candidate-only artifacts passed published readiness")

    saved_guard_git_output = globals()["git_output"]
    saved_guard_gh_json = globals()["gh_json"]
    saved_guard_gh_api_json = globals()["gh_api_json"]
    saved_guard_default_branch_head = globals()["default_branch_head"]
    guard_mode = "clean"
    guard_sha = "e" * 40

    with tempfile.TemporaryDirectory() as temporary:
        git_fixture = Path(temporary)
        subprocess.run(
            ["git", "init", "-q"], cwd=git_fixture, check=True, timeout=30
        )
        (git_fixture / "tracked.md").write_text("published\n", encoding="utf-8")
        subprocess.run(
            ["git", "-C", str(git_fixture), "add", "tracked.md"],
            check=True,
            timeout=30,
        )
        git_environment = os.environ.copy()
        git_environment.update(
            {
                "GIT_AUTHOR_NAME": "ProjectAtlas self-test",
                "GIT_AUTHOR_EMAIL": "self-test@example.invalid",
                "GIT_COMMITTER_NAME": "ProjectAtlas self-test",
                "GIT_COMMITTER_EMAIL": "self-test@example.invalid",
            }
        )
        subprocess.run(
            ["git", "-C", str(git_fixture), "commit", "-qm", "fixture"],
            check=True,
            timeout=30,
            env=git_environment,
        )
        (git_fixture / ".projectatlas").mkdir()
        (git_fixture / ".projectatlas" / "issue-manage-worktree-atlas-continuity.md").write_text(
            "local note\n", encoding="utf-8"
        )
        assert saved_guard_git_output(
            git_fixture, ["status", "--porcelain=v1", "--untracked-files=no"]
        ) == ""

    def fake_guard_git_output(root: Path, args: list[str]) -> str:
        if guard_mode == "git-failure":
            raise SystemExit("published readiness Git inspection failed: fixture")
        if args == ["rev-parse", "--show-toplevel"]:
            return str(self_test_root / "other") if guard_mode == "root-mismatch" else str(self_test_root)
        if args == ["status", "--porcelain=v1", "--untracked-files=no"]:
            return " M tracked.md" if guard_mode == "tracked-dirty" else ""
        if args == ["rev-parse", "HEAD"]:
            if guard_mode == "head-malformed":
                return "not-a-sha"
            return "d" * 40 if guard_mode == "stale" else guard_sha
        raise AssertionError(f"unexpected published Git fixture command: {args}")

    def fake_guard_gh_json(args: list[str]) -> object:
        if guard_mode == "default-unavailable":
            raise SystemExit("default branch unavailable")
        if guard_mode == "default-missing":
            return {}
        if guard_mode == "default-malformed":
            return {"defaultBranchRef": {"name": 7}}
        return {"defaultBranchRef": {"name": "main"}}

    def fake_guard_default_branch_head(repo: str, branch: str) -> str:
        if guard_mode == "live-unavailable":
            raise SystemExit("default branch ref unavailable")
        return guard_sha

    globals()["git_output"] = fake_guard_git_output
    globals()["gh_json"] = fake_guard_gh_json
    globals()["default_branch_head"] = fake_guard_default_branch_head
    try:
        assert require_published_snapshot(
            "owner/repo", self_test_root
        ) == PublishedSnapshot("main", guard_sha)
        for failure_mode, expected in (
            ("root-mismatch", "addressed Git checkout"),
            ("head-malformed", "HEAD was malformed"),
            ("stale", "does not equal live default branch"),
            ("tracked-dirty", "tracked modifications"),
            ("default-missing", "identity was malformed"),
            ("default-malformed", "identity was malformed"),
            ("default-unavailable", "default branch unavailable"),
            ("live-unavailable", "default branch ref unavailable"),
            ("git-failure", "Git inspection failed"),
        ):
            guard_mode = failure_mode
            try:
                require_published_snapshot("owner/repo", self_test_root)
            except SystemExit as error:
                assert expected in str(error)
            else:
                raise AssertionError(
                    f"published readiness fixture {failure_mode} was accepted"
                )
        guard_mode = "clean"
    finally:
        globals()["git_output"] = saved_guard_git_output
        globals()["gh_json"] = saved_guard_gh_json
        globals()["default_branch_head"] = saved_guard_default_branch_head
        globals()["gh_api_json"] = saved_guard_gh_api_json

    saved_git_subprocess_run = subprocess.run
    try:
        for failure, expected in (
            (
                lambda: (_ for _ in ()).throw(
                    subprocess.TimeoutExpired(["git"], 120)
                ),
                "Git inspection failed",
            ),
            (
                lambda: (_ for _ in ()).throw(OSError("fixture OS failure")),
                "Git inspection failed",
            ),
        ):
            subprocess.run = lambda *args, _failure=failure, **kwargs: _failure()
            try:
                saved_guard_git_output(self_test_root, ["rev-parse", "HEAD"])
            except SystemExit as error:
                assert expected in str(error)
            else:
                raise AssertionError("Git exception did not fail published readiness")

        class FailedGitProcess:
            returncode = 1
            stdout = ""
            stderr = "fixture process failure"

        subprocess.run = lambda *args, **kwargs: FailedGitProcess()
        try:
            saved_guard_git_output(self_test_root, ["rev-parse", "HEAD"])
        except SystemExit as error:
            assert "fixture process failure" in str(error)
        else:
            raise AssertionError("Git process failure did not fail published readiness")
    finally:
        subprocess.run = saved_git_subprocess_run

    malformed_refs = [
        [],
        {"object": {}},
        {"object": {"sha": "malformed"}},
    ]
    for malformed_ref in malformed_refs:
        globals()["gh_api_json"] = lambda args, payload=malformed_ref: payload
        try:
            default_branch_head("owner/repo", "main")
        except SystemExit as error:
            assert "default branch ref" in str(error)
        else:
            raise AssertionError("malformed default branch ref was accepted")
    globals()["gh_api_json"] = saved_guard_gh_api_json

    saved_planning_gh_json = globals()["gh_json"]
    saved_planning_gh_api_json = globals()["gh_api_json"]
    saved_planning_guard = globals()["require_published_snapshot"]
    planning_statuses: list[str] = []

    def fake_planning_gh_json(args: list[str]) -> object:
        if args[0] != "pr":
            raise AssertionError(f"unexpected planning PR call: {args}")
        fields = args[-1]
        if fields == "number,headRefOid,closingIssuesReferences,author,milestone":
            return {
                "number": 494,
                "headRefOid": "f" * 40,
                "closingIssuesReferences": [],
                "author": {"login": "owner"},
                "milestone": {"title": "v0.5.0-00"},
            }
        if fields == "number,headRefOid,closingIssuesReferences,id,autoMergeRequest,milestone,body":
            return {
                "number": 494,
                "headRefOid": "f" * 40,
                "closingIssuesReferences": [],
                "id": "PR_494",
                "autoMergeRequest": None,
                "milestone": {"title": "v0.5.0-00"},
                "body": "Relates to #12. The release-acceptance issue remains open and closes last; this PR implements no release feature or bug.",
            }
        raise AssertionError(f"unexpected planning PR fields: {fields}")

    def fake_planning_gh_api(args: list[str]) -> object:
        state = next(
            argument.split("=", 1)[1]
            for argument in args
            if argument.startswith("state=")
        )
        planning_statuses.append(state)
        return {}

    globals()["gh_json"] = fake_planning_gh_json
    globals()["gh_api_json"] = fake_planning_gh_api
    globals()["require_published_snapshot"] = lambda *args, **kwargs: (
        (_ for _ in ()).throw(AssertionError("planning PR unexpectedly required publication"))
    )
    try:
        publish_implementation_status_for_pr(
            "owner/repo", 494, self_test_root, self_test_root / "issue-map.json"
        )
        assert planning_statuses == ["pending", "success"]
    finally:
        globals()["gh_json"] = saved_planning_gh_json
        globals()["gh_api_json"] = saved_planning_gh_api_json
        globals()["require_published_snapshot"] = saved_planning_guard

    saved_merge_gh_json = globals()["gh_json"]
    saved_merge_gh_api_json = globals()["gh_api_json"]
    saved_merge_graph_failures = globals()["release_graph_failures"]
    saved_merge_guard = globals()["require_published_snapshot"]
    saved_sleep = time.sleep
    merge_sha = "a" * 40
    current_merge_head = merge_sha
    merge_state = "OPEN"
    merge_mode = "success"
    merge_statuses: list[tuple[str, str, str]] = []
    enable_calls = 0
    disable_calls = 0
    merge_graph_calls = 0
    default_branch_reads = 0
    review_thread_mode = "empty"
    merge_pr_reads = 0
    merge_reference = {**local_identity, "number": 10}

    def fake_merge_gh_json(args: list[str]) -> object:
        nonlocal current_merge_head, merge_state, merge_mode, merge_pr_reads
        if args[0] == "repo":
            repository = {
                "defaultBranchRef": {"name": "main"},
                "allowAutoMerge": merge_mode != "no-auto-merge",
                "name": "repo",
                "full_name": "owner/repo",
            }
            if merge_mode == "missing-owner":
                return repository
            repository["owner"] = (
                "owner"
                if merge_mode == "malformed-owner"
                else {
                    "login": "other" if merge_mode == "owner-mismatch" else "owner",
                    "type": "Organization" if merge_mode == "org-owner" else "User",
                }
            )
            return repository
        if args[0] == "issue":
            return {
                "number": int(args[2]),
                "milestone": {"title": "v1.2.3-00"},
            }
        if args[0] != "pr":
            raise AssertionError(f"unexpected merge gh call: {args}")
        fields = args[-1] if "--json" in args else ""
        if fields == "headRefOid":
            return {"headRefOid": current_merge_head}
        if fields != "state,headRefOid,baseRefName,mergeCommit":
            merge_pr_reads += 1
        if merge_mode == "missing-milestone":
            milestone = None
        elif merge_mode == "malformed-milestone":
            milestone = "v1.2.3-00"
        elif merge_mode in {"wrong-milestone", "final-milestone-drift"} and (
            merge_mode == "wrong-milestone" or merge_pr_reads >= 4
        ):
            milestone = {"title": "v9.9.9-00"}
        else:
            milestone = {"title": "v1.2.3-00"}
        planning = merge_mode in {
            "planning",
            "planning-body-drift",
            "dependabot-planning",
        }
        planning_body = (
            "Relates to #12. The release-acceptance issue remains open and closes last; "
            "this PR implements no release feature or bug."
        )
        if merge_mode == "planning-body-drift" and merge_pr_reads >= 4:
            planning_body = "Relates to #10. The owner relation changed before final read-back."
        return {
            "number": 494,
            "state": merge_state,
            "isDraft": merge_mode == "draft",
            "baseRefName": "main",
            "headRefOid": current_merge_head,
            "mergeable": "CONFLICTING" if merge_mode == "unmergeable" else "MERGEABLE",
            "mergeCommit": {"oid": "c" * 40} if merge_state == "MERGED" else None,
            "id": "PR_node_494",
            "author": {
                "login": "dependabot[bot]"
                if merge_mode == "dependabot-planning"
                else "owner"
            },
            "closingIssuesReferences": [] if planning else [merge_reference],
            "autoMergeRequest": {"id": "AUTO_494"} if planning else None,
            "milestone": milestone,
            "body": (
                planning_body
                if planning
                else ""
            ),
            "reviewDecision": {
                "review-failure": "CHANGES_REQUESTED",
                "review-no-decision": None,
            }.get(merge_mode, "APPROVED"),
            "reviews": [],
        }

    def fake_merge_gh_api(args: list[str]) -> object:
        nonlocal merge_state, merge_graph_calls, merge_mode, current_merge_head, enable_calls, disable_calls, default_branch_reads, review_thread_mode
        joined = " ".join(args)
        if "/statuses/" in joined and "commits/" not in joined:
            state = next(
                argument.split("=", 1)[1]
                for argument in args
                if argument.startswith("state=")
            )
            context = next(
                argument.split("=", 1)[1]
                for argument in args
                if argument.startswith("context=")
            )
            merge_statuses.append((state, context, args[2]))
            return {}
        if joined.startswith("repos/owner/repo --method GET"):
            if merge_mode == "malformed-repository":
                return []
            repository = {
                "name": "repo",
                "full_name": "owner/repo",
                "owner": {
                    "login": "other" if merge_mode == "owner-mismatch" else "owner",
                    "type": "Organization" if merge_mode == "org-owner" else "User",
                },
            }
            if merge_mode == "missing-owner":
                repository.pop("owner")
            elif merge_mode == "malformed-owner":
                repository["owner"] = "owner"
            return repository
        if "/git/ref/heads/main" in joined:
            default_branch_reads += 1
            if merge_mode == "published-drift-preflight":
                return {"object": {"sha": "d" * 40}}
            if merge_mode == "published-drift-final" and default_branch_reads >= 2:
                return {"object": {"sha": "d" * 40}}
            return {"object": {"sha": "b" * 40}}
        if "/branches/main/protection" in joined and "required_status_checks/contexts" not in joined:
            if merge_mode == "malformed-protection":
                return {
                    "required_pull_request_reviews": "malformed",
                    "required_status_checks": {"strict": True},
                }
            checks = []
            if merge_mode != "missing-merge-context":
                checks.append(
                    {
                        "context": MERGE_AUTHORIZATION_STATUS_CONTEXT,
                        "app_id": 999 if merge_mode == "wrong-merge-app" else GITHUB_ACTIONS_APP_ID,
                    }
                )
            checks.append({"context": "verify", "app_id": GITHUB_ACTIONS_APP_ID})
            review_policy = None
            if merge_mode == "required-approval":
                review_policy = {"required_approving_review_count": 1}
            elif merge_mode == "malformed-approval-policy":
                review_policy = {"required_approving_review_count": "1"}
            return {
                "required_pull_request_reviews": review_policy,
                "required_status_checks": {
                    "strict": merge_mode != "non-strict",
                    "checks": checks,
                }
            }
        if "required_status_checks/contexts" in joined:
            return [{"context": "verify"}]
        if "/commits/" in joined and "/statuses" in joined:
            return []
        if "/check-runs" in joined:
            return {
                "check_runs": [
                    {
                        "name": "verify",
                        "conclusion": "success",
                        "app": {"id": 999 if merge_mode == "wrong-check" else GITHUB_ACTIONS_APP_ID},
                    }
                ]
            }
        if "/reviews" in joined:
            return (
                [{"state": "CHANGES_REQUESTED"}]
                if merge_mode in {"review-failure", "review-history-approved"}
                else []
            )
        if "/collaborators" in joined:
            owner = {"login": "owner", "type": "User", "permissions": {"admin": True}}
            if merge_mode == "malformed-collaborators":
                return [{"login": "owner", "type": "User"}]
            if merge_mode == "collaborator-owner-mismatch":
                owner["login"] = "other"
            if merge_mode in {"extra-collaborator", "collaborator-truncation"}:
                return [owner, {"login": "extra", "type": "User", "permissions": {"push": True}}]
            return [owner]
        if args[0] == "graphql":
            query = next(argument for argument in args if argument.startswith("query="))
            if "reviewThreads" in query:
                if review_thread_mode == "malformed":
                    return {"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [], "pageInfo": {}}}}}}
                if review_thread_mode == "malformed-nodes":
                    return {"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": {}, "pageInfo": {"hasNextPage": False}}}}}}
                count = {
                    "below": MAX_REVIEWS - 1,
                    "exact": MAX_REVIEWS,
                    "exact-truncated": MAX_REVIEWS,
                    "overflow": MAX_REVIEWS + 1,
                }.get(review_thread_mode, 0)
                return {
                    "data": {
                        "repository": {
                            "pullRequest": {
                                "reviewThreads": {
                                    "nodes": [{"isResolved": True}] * count,
                                    "pageInfo": {
                                        "hasNextPage": review_thread_mode == "exact-truncated"
                                    },
                                }
                            }
                        }
                    }
                }
            if "enablePullRequestAutoMerge" in query:
                enable_calls += 1
                if merge_mode == "enable-failure":
                    return {"errors": [{"message": "enable failed"}]}
                if merge_mode == "final-drift":
                    current_merge_head = "d" * 40
                if merge_mode not in {"final-drift", "timeout"}:
                    merge_state = "MERGED"
                return {
                    "data": {
                        "enablePullRequestAutoMerge": {
                            "pullRequest": {"autoMergeRequest": {"enabledAt": "now"}}
                        }
                    }
                }
            if "disablePullRequestAutoMerge" in query:
                disable_calls += 1
                return {"data": {"disablePullRequestAutoMerge": {"pullRequest": {"number": 494}}}}
            return {"data": {}}
        if "milestones" in joined or "/issues" in joined:
            merge_graph_calls += 1
        return []

    globals()["gh_json"] = fake_merge_gh_json
    globals()["gh_api_json"] = fake_merge_gh_api
    globals()["release_graph_failures"] = lambda *args, **kwargs: (
        ["graph drift"] if merge_mode == "graph-failure" else []
    )
    saved_merge_implementation_failures = globals()["implementation_reference_failures"]
    globals()["implementation_reference_failures"] = lambda *args, **kwargs: []
    globals()["require_published_snapshot"] = lambda *args, **kwargs: PublishedSnapshot(
        "main", "b" * 40
    )
    time.sleep = lambda _: None
    try:
        exact_planning_body = (
            "Relates to #492. The release-acceptance issue remains open and closes last; "
            "this PR implements no release feature or bug."
        )
        exact_owner, exact_graph, exact_failures = resolve_pull_request_owner(
            "owner/repo",
            {
                "closingIssuesReferences": [],
                "body": exact_planning_body,
                "milestone": {"title": "v1.2.3-00"},
            },
            {"v1.2.3-00": ReleaseGraph("v1.2.3-00", 492, {492: ()})},
        )
        assert exact_owner == 492
        assert exact_graph == ReleaseGraph("v1.2.3-00", 492, {492: ()})
        assert exact_failures == []
        for invalid_owner in (
            {"closingIssuesReferences": [], "body": "Relates to #10", "milestone": {"title": "v1.2.3-00"}},
            {"closingIssuesReferences": [], "body": "Relates to #12\nRelates to #11", "milestone": {"title": "v1.2.3-00"}},
            {"closingIssuesReferences": [merge_reference], "body": exact_planning_body, "milestone": {"title": "v1.2.3-00"}},
            {"closingIssuesReferences": [], "body": "Relates to #", "milestone": {"title": "v1.2.3-00"}},
            {"closingIssuesReferences": [], "milestone": {"title": "v1.2.3-00"}},
            {"closingIssuesReferences": [merge_reference, merge_reference], "body": "", "milestone": {"title": "v1.2.3-00"}},
        ):
            _, _, owner_failures = resolve_pull_request_owner(
                "owner/repo", invalid_owner, valid_graphs
            )
            assert owner_failures
        for mode in ("below", "exact"):
            review_thread_mode = mode
            assert unresolved_review_failures("owner/repo", 494) == []
        for mode in ("exact-truncated", "overflow", "malformed", "malformed-nodes"):
            review_thread_mode = mode
            try:
                unresolved_review_failures("owner/repo", 494)
            except SystemExit as error:
                assert "review-thread" in str(error) or "bounded" in str(error)
            else:
                raise AssertionError(f"review-thread pagination mode {mode} was accepted")
        review_thread_mode = "empty"
        assert authorize_merge(
            "owner/repo",
            494,
            merge_sha,
            Path("."),
            {},
            valid_graphs,
            "merge-test-01",
            "owner",
            "owner",
        ) == "applied"
        assert [state for state, context, _ in merge_statuses if context == MERGE_AUTHORIZATION_STATUS_CONTEXT] == [
            "pending",
            "success",
        ]
        assert json.loads(merge_outcome("merge-test-01", "applied"))["event"] == DISPATCH_MERGE_EVENT
        merge_mode = "planning"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        merge_pr_reads = 0
        before_enable = enable_calls
        assert authorize_merge(
            "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
            "merge-planning", "owner", "owner"
        ) == "applied"
        assert enable_calls == before_enable + 1
        before_disable = disable_calls
        revoke_merge_authorization_for_pr(
            "owner/repo", 494, expected_head_sha=merge_sha, release_graphs=valid_graphs
        )
        assert disable_calls == before_disable + 1
        assert any(
            state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses
        )
        merge_mode = "dependabot-planning"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        merge_pr_reads = 0
        before_enable = enable_calls
        before_statuses = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-dependabot-planning", "owner", "owner"
            )
        except SystemExit as error:
            assert "Dependabot" in str(error)
        else:
            raise AssertionError("Dependabot planning PR was authorized")
        assert enable_calls == before_enable
        assert any(
            state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before_statuses:]
        )
        merge_mode = "wrong-milestone"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        before_enable = enable_calls
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-wrong-milestone", "owner", "owner"
            )
        except SystemExit as error:
            assert "milestone" in str(error)
        else:
            raise AssertionError("wrong PR milestone was accepted")
        assert enable_calls == before_enable
        for milestone_mode, milestone_value in (
            ("missing-milestone", None),
            ("malformed-milestone", "v1.2.3-00"),
        ):
            merge_mode = milestone_mode
            failures = pull_request_milestone_failures(
                "owner/repo",
                {"closingIssuesReferences": [merge_reference], "milestone": milestone_value, "body": ""},
                valid_graphs,
            )
            assert any("milestone" in failure for failure in failures)
        ambiguous_graphs = {
            **valid_graphs,
            "v9.9.9-00": ReleaseGraph(
                "v9.9.9-00", 12, {10: (11,), 11: (), 12: (10, 11)}
            ),
        }
        failures = pull_request_milestone_failures(
            "owner/repo",
            {"closingIssuesReferences": [merge_reference], "milestone": {"title": "v1.2.3-00"}, "body": ""},
            ambiguous_graphs,
        )
        assert any("exactly one release graph" in failure for failure in failures)
        merge_mode = "success"
        merge_pr_reads = 0
        assert authorize_merge(
            "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
            "merge-restored-milestone", "owner", "owner"
        ) == "applied"
        merge_mode = "final-milestone-drift"
        merge_pr_reads = 0
        merge_state = "OPEN"
        current_merge_head = merge_sha
        disable_calls = 0
        before_enable = enable_calls
        before_statuses = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-final-milestone-drift", "owner", "owner"
            )
        except SystemExit as error:
            assert "milestone" in str(error)
        else:
            raise AssertionError("final milestone movement was accepted")
        assert enable_calls == before_enable + 1
        assert disable_calls == 1
        assert any(
            state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before_statuses:]
        )
        merge_mode = "planning-body-drift"
        merge_pr_reads = 0
        merge_state = "OPEN"
        current_merge_head = merge_sha
        disable_calls = 0
        before_enable = enable_calls
        before_statuses = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-planning-body-drift", "owner", "owner"
            )
        except SystemExit as error:
            assert "body relationship" in str(error) or "owner" in str(error)
        else:
            raise AssertionError("final planning relationship movement was accepted")
        assert enable_calls == before_enable + 1
        assert disable_calls == 1
        assert any(
            state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before_statuses:]
        )
        merge_mode = "success"
        merge_pr_reads = 0
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-test-02", "other", "other"
            )
        except SystemExit as error:
            assert "actor and sender" in str(error)
        else:
            raise AssertionError("foreign merge dispatch actor was accepted")
        merge_state = "MERGED"
        assert authorize_merge(
            "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
            "merge-test-03", "owner", "owner"
        ) == "already-satisfied"
        current_merge_head = "b" * 40
        merge_state = "OPEN"
        before_stale = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-test-04", "owner", "owner"
            )
        except SystemExit as error:
            assert "head changed" in str(error) or "authorization" in str(error)
        else:
            raise AssertionError("stale merge head was authorized")
        assert not any(
            state == "success" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before_stale:]
        )
        current_merge_head = merge_sha
        for failure_mode in ("enable-failure", "final-drift", "timeout"):
            merge_mode = failure_mode
            merge_state = "OPEN"
            current_merge_head = merge_sha
            before = len(merge_statuses)
            try:
                authorize_merge(
                    "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                    f"merge-{failure_mode}", "owner", "owner"
                )
            except SystemExit:
                pass
            else:
                raise AssertionError(f"merge {failure_mode} was incorrectly authorized")
            dispositions = merge_statuses[before:]
            assert any(
                state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
                for state, context, _ in dispositions
            )
        for failure_mode in ("published-drift-preflight", "published-drift-final"):
            merge_mode = failure_mode
            merge_state = "OPEN"
            current_merge_head = merge_sha
            default_branch_reads = 0
            before_enable = enable_calls
            before = len(merge_statuses)
            try:
                authorize_merge(
                    "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                    f"merge-{failure_mode}", "owner", "owner"
                )
            except SystemExit as error:
                assert "default branch changed" in str(error)
            else:
                raise AssertionError(f"merge {failure_mode} was incorrectly authorized")
            assert any(
                state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
                for state, context, _ in merge_statuses[before:]
            )
            if failure_mode == "published-drift-preflight":
                assert enable_calls == before_enable
        assert disable_calls >= 2
        for policy_failure in (
            "missing-merge-context",
            "wrong-merge-app",
            "non-strict",
            "malformed-protection",
            "malformed-approval-policy",
            "no-auto-merge",
            "missing-owner",
            "malformed-repository",
            "malformed-owner",
            "org-owner",
            "owner-mismatch",
            "extra-collaborator",
            "collaborator-owner-mismatch",
            "malformed-collaborators",
            "collaborator-truncation",
        ):
            merge_mode = policy_failure
            merge_state = "OPEN"
            current_merge_head = merge_sha
            before_enable = enable_calls
            before = len(merge_statuses)
            try:
                authorize_merge(
                    "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                    f"merge-{policy_failure}", "owner", "owner"
                )
            except SystemExit:
                pass
            else:
                raise AssertionError(f"merge policy failure {policy_failure} was accepted")
            assert enable_calls == before_enable
            assert any(
                state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
                for state, context, _ in merge_statuses[before:]
            )
            assert not any(
                state == "success" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
                for state, context, _ in merge_statuses[before:]
            )
        merge_mode = "wrong-check"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        before = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-wrong-check", "owner", "owner"
            )
        except SystemExit:
            pass
        else:
            raise AssertionError("wrong-app required check was accepted")
        assert not any(
            state == "success" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before:]
        )
        merge_mode = "review-failure"
        before = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-review-failure", "owner", "owner"
            )
        except SystemExit:
            pass
        else:
            raise AssertionError("changes-requested review was accepted")
        assert not any(
            state == "success" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before:]
        )
        merge_mode = "graph-failure"
        before = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-graph-failure", "owner", "owner"
            )
        except SystemExit:
            pass
        else:
            raise AssertionError("release graph drift was accepted")
        assert not any(
            state == "success" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before:]
        )
        merge_mode = "draft"
        before = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-draft", "owner", "owner"
            )
        except SystemExit:
            pass
        else:
            raise AssertionError("draft PR was accepted")
        assert not any(
            state == "success" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before:]
        )
        merge_mode = "review-history-approved"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        assert authorize_merge(
            "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
            "merge-review-history-approved", "owner", "owner"
        ) == "applied"
        merge_mode = "review-no-decision"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        assert authorize_merge(
            "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
            "merge-review-no-decision", "owner", "owner"
        ) == "applied"
        merge_mode = "required-approval"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        before_enable = enable_calls
        before_statuses = len(merge_statuses)
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-required-approval", "owner", "owner"
            )
        except SystemExit as error:
            assert "zero approving reviews" in str(error)
        else:
            raise AssertionError("branch protection requiring approval was accepted")
        assert enable_calls == before_enable
        assert any(
            state == "failure" and context == MERGE_AUTHORIZATION_STATUS_CONTEXT
            for state, context, _ in merge_statuses[before_statuses:]
        )
        merge_mode = "malformed-approval-policy"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        before_enable = enable_calls
        try:
            authorize_merge(
                "owner/repo", 494, merge_sha, Path("."), {}, valid_graphs,
                "merge-malformed-approval-policy", "owner", "owner"
            )
        except SystemExit as error:
            assert "approving-review count" in str(error)
        else:
            raise AssertionError("malformed branch approval policy was accepted")
        assert enable_calls == before_enable
        merge_mode = "success"
        merge_state = "OPEN"
        current_merge_head = merge_sha
        globals()["gh_api_json"] = lambda args: [{"number": 1}] * 3
        try:
            bounded_api_collection([], 2, "test collection")
        except BaseException as error:
            assert "bounded" in str(error)
        else:
            raise AssertionError("bounded collection did not fail closed")
        globals()["gh_api_json"] = lambda args: {"check_runs": [{"name": "verify"}] * 3}
        try:
            bounded_api_object_collection([], "check_runs", 2, "test object collection")
        except BaseException as error:
            assert "bounded" in str(error)
        else:
            raise AssertionError("bounded object collection did not fail closed")
        globals()["gh_api_json"] = lambda args: ["verify"] * 3
        try:
            bounded_api_values([], 2, "test scalar collection")
        except BaseException as error:
            assert "bounded" in str(error)
        else:
            raise AssertionError("bounded scalar collection did not fail closed")
        globals()["gh_api_json"] = fake_merge_gh_api
        for malformed in (None, "", "short"):
            try:
                validate_request_id(malformed)
            except SystemExit:
                pass
            else:
                raise AssertionError("malformed dispatch request_id was accepted")
        for malformed_number, malformed_head in (
            (None, "a" * 40),
            ("not-a-number", "a" * 40),
            (494, "short"),
        ):
            try:
                validate_merge_request(malformed_number, malformed_head)
            except SystemExit:
                pass
            else:
                raise AssertionError("malformed merge dispatch payload was accepted")
        assert json.loads(relationship_outcome("rel-test-01", "failed"))["outcome"] == "failed"
    finally:
        globals()["gh_json"] = saved_merge_gh_json
        globals()["gh_api_json"] = saved_merge_gh_api_json
        globals()["release_graph_failures"] = saved_merge_graph_failures
        globals()["implementation_reference_failures"] = saved_merge_implementation_failures
        globals()["require_published_snapshot"] = saved_merge_guard
        time.sleep = saved_sleep
    print("issue checklist self-test passed")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="")
    parser.add_argument("--root", default=".")
    parser.add_argument("--issue-map", default="openspec/issue-map.json")
    parser.add_argument("--milestone", action="append", default=[])
    issue_mode = parser.add_mutually_exclusive_group()
    issue_mode.add_argument("--planned-issue", type=int)
    issue_mode.add_argument(
        "--implementation-issue",
        type=int,
        help="require every direct release-graph blocker to be closed",
    )
    parser.add_argument("--publish-implementation-status-for-pr", type=int)
    parser.add_argument("--expected-pr-head-sha")
    parser.add_argument("--revoke-merge-authorization-for-pr", type=int)
    parser.add_argument("--enforce-closed-issue-blockers", type=int)
    parser.add_argument("--mutate-native-relationship", action="store_true")
    parser.add_argument("--native-relationship-kind")
    parser.add_argument("--native-relationship-operation")
    parser.add_argument("--native-relationship-issue")
    parser.add_argument("--native-related-issue")
    parser.add_argument("--authorize-merge", action="store_true")
    parser.add_argument("--merge-pr-number")
    parser.add_argument("--merge-expected-head")
    parser.add_argument("--request-id")
    parser.add_argument("--dispatch-actor", default="")
    parser.add_argument("--event-sender", default="")
    parser.add_argument("--skip-openspec", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if not args.repo:
        raise SystemExit("--repo is required unless --self-test is used")

    request_id = validate_request_id(args.request_id) if args.request_id is not None else None
    if args.authorize_merge:
        if request_id is None:
            raise SystemExit("merge authorization requires a validated --request-id")
        try:
            number, expected_head = validate_merge_request(
                args.merge_pr_number, args.merge_expected_head
            )
            root = Path(args.root)
            published_snapshot = require_published_snapshot(args.repo, root)
            issue_map = load_issue_map(args.issue_map)
            release_graphs = load_release_graphs(args.issue_map, issue_map)
            outcome = authorize_merge(
                args.repo,
                number,
                expected_head,
                Path(args.root),
                issue_map,
                release_graphs,
                request_id,
                args.dispatch_actor,
                args.event_sender,
                published_snapshot=published_snapshot,
            )
        except BaseException:
            print(merge_outcome(request_id, "failed"))
            raise
        print(merge_outcome(request_id, outcome))
        return

    if args.enforce_closed_issue_blockers is not None:
        require_published_snapshot(args.repo, Path(args.root))
        issue_map = load_issue_map(args.issue_map)
        release_graphs = load_release_graphs(args.issue_map, issue_map)
        enforce_closed_issue_blockers(
            args.repo, args.enforce_closed_issue_blockers, release_graphs
        )
        return

    if args.revoke_merge_authorization_for_pr is not None:
        root = Path(args.root)
        require_published_snapshot(args.repo, root)
        issue_map = load_issue_map(args.issue_map)
        release_graphs = load_release_graphs(args.issue_map, issue_map)
        revoke_merge_authorization_for_pr(
            args.repo,
            args.revoke_merge_authorization_for_pr,
            args.expected_pr_head_sha,
            release_graphs=release_graphs,
        )
        return

    if args.mutate_native_relationship:
        if request_id is None:
            raise SystemExit("native relationship mutation requires a validated --request-id")
        try:
            issue, related_issue = validate_native_relationship_request(
                args.native_relationship_kind,
                args.native_relationship_operation,
                args.native_relationship_issue,
                args.native_related_issue,
            )
            root = Path(args.root)
            published_snapshot = require_published_snapshot(args.repo, root)
            issue_map = load_issue_map(args.issue_map)
            release_graphs = load_release_graphs(args.issue_map, issue_map)
            mutate_native_relationship_and_revalidate(
                args.repo,
                issue,
                related_issue,
                args.native_relationship_kind,
                args.native_relationship_operation,
                issue_map,
                release_graphs,
                request_id,
                root=root,
                published_snapshot=published_snapshot,
            )
        except BaseException:
            print(relationship_outcome(request_id, "failed"))
            raise
        return

    if args.publish_implementation_status_for_pr is not None:
        publish_implementation_status_for_pr(
            args.repo,
            args.publish_implementation_status_for_pr,
            Path(args.root),
            args.issue_map,
            expected_pr_head_sha=args.expected_pr_head_sha,
        )
        return

    root = Path(args.root)
    failures: list[str] = []
    if (
        args.planned_issue is not None
        or args.implementation_issue is not None
        or args.milestone
    ):
        require_published_snapshot(args.repo, root)
    issue_map = load_issue_map(args.issue_map)
    release_graphs = load_release_graphs(args.issue_map, issue_map)
    issue_number = args.planned_issue or args.implementation_issue
    target_issue: dict[str, object] | None = None
    target_graph: ReleaseGraph | None = None
    if issue_number is not None:
        target_issue = issue_payload(args.repo, issue_number)
        target_graph, target_failures = target_graph_failures(
            target_issue, release_graphs
        )
        failures.extend(target_failures)
    if not args.skip_openspec:
        failures.extend(
            check_openspec_tasks(
                args.repo,
                root,
                issue_map,
                planned_issue=args.planned_issue
                if args.planned_issue is not None
                else args.implementation_issue,
            )
        )
    if target_issue is not None:
        failures.extend(
            planned_issue_failures(
                target_issue, issue_map, root
            )
        )
    if args.implementation_issue is not None and target_issue is not None:
        failures.extend(
            implementation_issue_failures(args.repo, target_issue, release_graphs)
        )
    graph_milestones = set(args.milestone)
    if target_graph is not None:
        graph_milestones.add(target_graph.milestone)
    elif not graph_milestones and target_issue is not None:
        milestone = target_issue.get("milestone")
        if isinstance(milestone, dict) and isinstance(milestone.get("title"), str):
            graph_milestones.add(milestone["title"])
    failures.extend(
        release_graph_failures(
            args.repo,
            release_graphs,
            issue_map,
            graph_milestones or None,
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
