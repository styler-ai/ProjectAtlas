"""Verify GitHub issue checklists mirror OpenSpec tasks."""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from urllib.parse import quote, unquote, urlsplit


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
class IssueOpsSnapshot:
    """One bounded live snapshot shared by all affected PR evaluations."""

    pull_requests: tuple[dict[str, object], ...]
    issue_payloads: dict[int, dict[str, object]]
    graph_failures: dict[str, tuple[str, ...]]
    selection_failures: tuple[str, ...]
    overflow_implementation_prs: tuple[dict[str, object], ...] = ()
    evicted_implementation_prs: tuple[dict[str, object], ...] = ()
    admission_ready: bool = True


@dataclass(frozen=True)
class ImplementationStatusCandidate:
    """One validated PR head prepared for a two-phase status refresh."""

    pull_request: dict[str, object]
    number: int
    expected_sha: str


@dataclass(frozen=True)
class ImplementationAdmission:
    """One deterministic bounded set of active implementation PRs."""

    admitted: tuple[dict[str, object], ...]
    overflow: tuple[dict[str, object], ...]
    evicted: tuple[dict[str, object], ...]
    failures: tuple[str, ...]
    ready: bool


class CandidateHeadChanged(SystemExit):
    """One selected PR changed heads after the immutable snapshot."""

    def __init__(self, number: int, expected_sha: str, live_sha: str) -> None:
        self.live_sha = live_sha
        super().__init__(
            f"PR #{number} head changed before status publication: "
            f"expected {expected_sha}, found {live_sha}"
        )


MAX_OPEN_PULL_REQUESTS = 1000
MAX_ACTIVE_IMPLEMENTATION_PRS = 16
NATIVE_RELATION_KINDS = {"blocked_by", "sub_issue"}
NATIVE_RELATION_OPERATIONS = {"add", "remove"}
STATUS_WORKERS = 8
ISSUEOPS_WORKFLOW_PATH = ".github/workflows/issueops.yml"
ISSUEOPS_EVENTS = {"issues", "repository_dispatch"}
REQUEST_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{7,63}$")
ISSUEOPS_ADMISSION_LABEL = "issueops-admitted"
MAX_ADMISSION_LABEL_MUTATIONS = MAX_ACTIVE_IMPLEMENTATION_PRS
MAX_ADMISSION_REPAIR_STATUS_WRITES = 1
MAX_GRAPH_REQUESTS_PER_EVENT = 256
MAX_STATUS_WRITES_PER_EVENT = (
    (MAX_ACTIVE_IMPLEMENTATION_PRS * 3) + MAX_ADMISSION_REPAIR_STATUS_WRITES
)
MAX_GENERATION_READS_PER_EVENT = (
    MAX_ACTIVE_IMPLEMENTATION_PRS + STATUS_WORKERS - 1
) // STATUS_WORKERS
MAX_CONTENT_WRITES_PER_MINUTE = 80
MAX_GITHUB_REQUESTS_PER_HOUR = 1000


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


def newer_issueops_run_exists(repo: str, current_run_id: int) -> bool:
    """Reject success when a newer trusted IssueOps event generation exists."""

    current_run_id = positive_issue(current_run_id, "IssueOps workflow run id")
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            f"repos/{owner}/{name}/actions/workflows/{Path(ISSUEOPS_WORKFLOW_PATH).name}/runs",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-F",
            "per_page=100",
        ]
    )
    if not isinstance(payload, dict) or not isinstance(payload.get("workflow_runs"), list):
        raise SystemExit("GitHub IssueOps workflow-runs response was malformed")
    current_seen = False
    for item in payload["workflow_runs"]:
        if not isinstance(item, dict):
            raise SystemExit("GitHub IssueOps workflow-runs item was malformed")
        if item.get("path") != ISSUEOPS_WORKFLOW_PATH:
            raise SystemExit("GitHub IssueOps workflow-runs response had an unexpected path")
        run_id = positive_issue(item.get("id"), "IssueOps workflow run id")
        event = item.get("event")
        if event not in ISSUEOPS_EVENTS:
            continue
        if run_id == current_run_id:
            current_seen = True
        if run_id > current_run_id:
            return True
    if not current_seen:
        raise SystemExit("current IssueOps workflow run was missing from workflow-runs response")
    return False


def clean(text: str) -> str:
    return " ".join((text or "").replace("\r", "").split())


def validate_request_id(value: object) -> str:
    """Validate an operator correlation token without treating it as authority."""

    if not isinstance(value, str) or REQUEST_ID_RE.fullmatch(value) is None:
        raise SystemExit(
            "repository_dispatch request_id must be 8-64 ASCII letters, digits, "
            "periods, underscores, or hyphens, beginning with a letter or digit"
        )
    return value


def validate_native_relationship_request(
    relation_kind: object,
    operation: object,
    issue: object,
    related_issue: object,
) -> tuple[int, int]:
    """Validate all relationship fields before any read, label, or write side effect."""

    if not isinstance(relation_kind, str) or relation_kind not in NATIVE_RELATION_KINDS:
        raise SystemExit(
            f"native relation kind must be one of {sorted(NATIVE_RELATION_KINDS)}, "
            f"got {relation_kind!r}"
        )
    if not isinstance(operation, str) or operation not in NATIVE_RELATION_OPERATIONS:
        raise SystemExit(
            f"native relation operation must be one of {sorted(NATIVE_RELATION_OPERATIONS)}, "
            f"got {operation!r}"
        )
    if issue is None or related_issue is None:
        raise SystemExit(
            "native relationship mutation requires kind, operation, issue, and related issue"
        )
    def relationship_issue_number(value: object, label: str) -> int:
        if isinstance(value, str) and re.fullmatch(r"[1-9][0-9]*", value):
            value = int(value)
        return positive_issue(value, label)

    issue_number = relationship_issue_number(issue, "issue number")
    related_number = relationship_issue_number(related_issue, "related issue number")
    if issue_number == related_number:
        raise SystemExit("native relationship cannot relate an issue to itself")
    return issue_number, related_number


def relationship_outcome(request_id: str, outcome: str) -> str:
    """Encode one parseable relationship-dispatch lifecycle outcome."""

    request_id = validate_request_id(request_id)
    if outcome not in {"applied", "already-satisfied", "failed"}:
        raise SystemExit(f"unknown relationship dispatch outcome {outcome!r}")
    return json.dumps(
        {"event": "issueops_relationship", "outcome": outcome, "request_id": request_id},
        sort_keys=True,
        separators=(",", ":"),
    )


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
    issue_payloads: dict[int, dict[str, object]] | None = None,
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
            payload = (
                issue_payloads[owner.issue]
                if issue_payloads is not None and owner.issue in issue_payloads
                else issue_payload(repo, owner.issue)
            )
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
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
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
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-F",
            "state=all",
            "-F",
            f"milestone={number}",
            "-F",
            "per_page=100",
        ]
    )
    return [item for item in flatten_paginated_response(payload) if "pull_request" not in item]


GITHUB_API_VERSION = "2026-03-10"
IMPLEMENTATION_STATUS_CONTEXT = "issueops-implementation"


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
        repository_name = repository.get("name")
        repository_owner = repository.get("owner")
        if repository_name is not None or repository_owner is not None:
            if not isinstance(repository_name, str) or not isinstance(repository_owner, dict):
                raise SystemExit(
                    f"GitHub returned {label} with malformed repository identity"
                )
            owner_login = repository_owner.get("login")
            if not isinstance(owner_login, str):
                raise SystemExit(
                    f"GitHub returned {label} with malformed repository identity"
                )
            identities.append(
                f"{owner_login}/{repository_name}".casefold() == expected_full_name
            )
    if not identities or not all(identities):
        raise SystemExit(
            f"GitHub returned {label} without the exact {repo} repository identity"
        )
    return positive_issue(payload.get("number"), label)


def native_blocked_by(repo: str, issue: int) -> set[int]:
    """Read GitHub's native blocked-by edges for one issue."""

    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            f"repos/{owner}/{name}/issues/{issue}/dependencies/blocked_by",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-F",
            "per_page=100",
        ]
    )
    dependencies = flatten_paginated_response(payload)
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
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            f"repos/{owner}/{name}/issues/{issue}/sub_issues",
            "--method",
            "GET",
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            "-F",
            "per_page=100",
        ]
    )
    children = flatten_paginated_response(payload)
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
    """Read one repository-local issue id for a native relation mutation."""

    issue = positive_issue(issue, "issue number")
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            f"repos/{owner}/{name}/issues/{issue}",
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
            f"GitHub returned {label} #{returned_number}, expected repository-local issue #{issue}"
        )
    return positive_issue(payload.get("id"), f"{label} id")


def mutate_native_relationship(
    repo: str,
    issue: int,
    related_issue: int,
    relation_kind: str,
    operation: str,
) -> bool:
    """Apply one idempotent native edge and require an exact read-back."""

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
            if desired:
                path = f"repos/{owner}/{name}/issues/{issue}/dependencies/blocked_by"
                field = "issue_id"
            else:
                path = (
                    f"repos/{owner}/{name}/issues/{issue}/dependencies/blocked_by/{related_id}"
                )
                field = None
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
        method = "POST" if desired else "DELETE"
        request = [
            "gh",
            "api",
            path,
            "--method",
            method,
            "-H",
            f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
        ]
        if field is not None:
            request.extend(["-F", f"{field}={related_id}"])
        run(request)
    actual = read_relation(repo, issue)
    if (related_issue in actual) != desired:
        raise SystemExit(
            f"native {relation_kind} {operation} read-back did not produce the requested edge "
            f"#{issue} -> #{related_issue}"
        )
    return changed


def mutate_native_relationship_and_revalidate(
    repo: str,
    issue: int,
    related_issue: int,
    relation_kind: str,
    operation: str,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    run_id: int | None = None,
    request_id: str | None = None,
) -> None:
    """Mutate one authorized edge, then reconcile every declared release graph."""

    issue, related_issue = validate_native_relationship_request(
        relation_kind, operation, issue, related_issue
    )
    snapshot, affected = build_issueops_snapshot(
        repo,
        issue,
        issue_map,
        release_graphs,
        reconcile_all_release_graphs=True,
        enforce_admission=True,
    )
    candidates, candidate_failures = prepare_implementation_status_candidates(affected)
    admission_failures = publish_admission_failure_statuses(
        repo, snapshot.evicted_implementation_prs
    )
    if snapshot.selection_failures or candidate_failures or admission_failures:
        failures = list(snapshot.selection_failures) + candidate_failures + admission_failures
        raise SystemExit(
            "relationship dispatch preflight failed:\n" + "\n".join(failures)
        )
    pending_failures = publish_pending_statuses(repo, candidates)
    if pending_failures:
        raise SystemExit(
            "relationship dispatch pending phase failed:\n"
            + "\n".join(pending_failures)
        )
    changed = mutate_native_relationship(repo, issue, related_issue, relation_kind, operation)
    post_mutation_snapshot = refresh_snapshot_graph_failures(
        repo, issue_map, release_graphs, snapshot
    )
    failures = finalize_implementation_statuses(
        repo,
        root,
        issue_map,
        release_graphs,
        post_mutation_snapshot,
        candidates,
        run_id=run_id,
    )
    if failures:
        raise SystemExit(
            "affected implementation status publication failed:\n"
            + "\n".join(failures)
        )
    if request_id is not None:
        print(
            relationship_outcome(
                request_id, "applied" if changed else "already-satisfied"
            )
        )


def open_pull_requests_snapshot(repo: str) -> tuple[dict[str, object], ...]:
    """Read one bounded open-PR snapshot and reject truncation at the cap."""

    pull_requests = gh_json(
        [
            "pr",
            "list",
            "-R",
            repo,
            "--state",
            "open",
            "--limit",
            str(MAX_OPEN_PULL_REQUESTS + 1),
            "--json",
            "number,headRefOid,closingIssuesReferences,author,labels",
        ]
    )
    if not isinstance(pull_requests, list):
        raise SystemExit("GitHub open pull-request response must be an array")
    if len(pull_requests) > MAX_OPEN_PULL_REQUESTS:
        raise SystemExit(
            "open pull-request snapshot exceeded the bounded limit; "
            "refusing to publish incomplete affected statuses"
        )
    if not all(isinstance(item, dict) for item in pull_requests):
        raise SystemExit("GitHub open pull-request response contained a non-object")
    return tuple(pull_requests)


def graph_request_budget(release_graphs: dict[str, ReleaseGraph]) -> int:
    """Estimate the bounded graph/issue reads used by one global refresh."""

    return sum(4 * len(graph.blocked_by) + 8 for graph in release_graphs.values())


def issueops_request_budget(release_graphs: dict[str, ReleaseGraph]) -> dict[str, int]:
    """Model one worst-case admitted refresh against GitHub request ceilings."""

    label_writes = MAX_ADMISSION_LABEL_MUTATIONS + 1
    status_writes = MAX_STATUS_WRITES_PER_EVENT
    return {
        "discovery_reads": 1,
        "head_reads": (MAX_ACTIVE_IMPLEMENTATION_PRS * 2)
        + MAX_ADMISSION_REPAIR_STATUS_WRITES,
        "generation_reads": MAX_GENERATION_READS_PER_EVENT,
        "graph_reads": min(graph_request_budget(release_graphs), MAX_GRAPH_REQUESTS_PER_EVENT),
        "status_writes": status_writes,
        "label_writes": label_writes,
        "content_writes": status_writes + label_writes,
        "request_total": (
            1
            + (MAX_ACTIVE_IMPLEMENTATION_PRS * 2)
            + MAX_ADMISSION_REPAIR_STATUS_WRITES
            + MAX_GENERATION_READS_PER_EVENT
            + min(graph_request_budget(release_graphs), MAX_GRAPH_REQUESTS_PER_EVENT)
            + status_writes
            + label_writes
        ),
    }


def implementation_prs_for_release_graphs(
    repo: str,
    pull_requests: tuple[dict[str, object], ...],
    graphs: dict[str, ReleaseGraph],
) -> tuple[list[dict[str, object]], list[str]]:
    """Classify every open PR that closes an issue in a declared release graph."""

    graph_issues = sorted(
        {issue for graph in graphs.values() for issue in graph.blocked_by}
    )
    if not graph_issues:
        return [], []
    return affected_implementation_prs(
        repo,
        graph_issues[0],
        pull_requests,
        graphs,
        tuple(graph_issues[1:]),
    )


def pull_request_has_admission_label(pull_request: dict[str, object]) -> bool:
    """Return whether a PR carries the default-branch-owned admission marker."""

    labels = pull_request.get("labels")
    if not isinstance(labels, list):
        return False
    return any(
        isinstance(label, dict) and label.get("name") == ISSUEOPS_ADMISSION_LABEL
        for label in labels
    )


def ensure_admission_label(repo: str) -> None:
    """Create the repository-owned admission label once, with pinned API calls."""

    owner, name = repo_parts(repo)
    label_path = (
        f"repos/{owner}/{name}/labels/{quote(ISSUEOPS_ADMISSION_LABEL, safe='')}"
    )
    try:
        label = gh_api_json(
            [
                label_path,
                "--method",
                "GET",
                "-H",
                f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            ]
        )
    except SystemExit as error:
        if "404" not in str(error):
            raise
        label = gh_api_json(
            [
                f"repos/{owner}/{name}/labels",
                "--method",
                "POST",
                "-H",
                f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
                "-f",
                f"name={ISSUEOPS_ADMISSION_LABEL}",
                "-f",
                "color=5319E7",
                "-f",
                "description=IssueOps status admission slot",
            ]
        )
    if not isinstance(label, dict):
        raise SystemExit("IssueOps admission label response must be an object")


def reconcile_admission_labels(
    repo: str, implementation_prs: list[dict[str, object]]
) -> ImplementationAdmission:
    """Maintain the bounded native admission marker from trusted default-branch code."""

    ordered = sorted(
        implementation_prs,
        key=lambda pull_request: positive_issue(
            pull_request.get("number"), "implementation pull request number"
        ),
    )
    admitted = tuple(ordered[:MAX_ACTIVE_IMPLEMENTATION_PRS])
    overflow = tuple(ordered[MAX_ACTIVE_IMPLEMENTATION_PRS:])
    desired = {
        positive_issue(item.get("number"), "implementation pull request number")
        for item in admitted
    }
    mutations: list[tuple[dict[str, object], bool]] = []
    evicted: list[dict[str, object]] = []
    for pull_request in ordered:
        number = positive_issue(
            pull_request.get("number"), "implementation pull request number"
        )
        has_label = pull_request_has_admission_label(pull_request)
        should_have_label = number in desired
        if has_label != should_have_label:
            mutations.append((pull_request, should_have_label))
            if has_label and not should_have_label:
                evicted.append(pull_request)
    if len(evicted) > MAX_ADMISSION_REPAIR_STATUS_WRITES:
        return ImplementationAdmission(
            admitted=admitted,
            overflow=overflow,
            evicted=(),
            failures=(
                "IssueOps admission label drift would require more than the bounded "
                f"{MAX_ADMISSION_REPAIR_STATUS_WRITES}-PR status repair"
            ),
            ready=False,
        )
    if len(mutations) > MAX_ADMISSION_LABEL_MUTATIONS:
        return ImplementationAdmission(
            admitted=admitted,
            overflow=overflow,
            evicted=tuple(evicted),
            failures=(
                "IssueOps admission label drift exceeded the bounded repair budget; "
                "refusing to certify implementation statuses"
            ),
            ready=False,
        )
    try:
        if ordered:
            ensure_admission_label(repo)
        owner, name = repo_parts(repo)
        for pull_request, should_have_label in mutations:
            number = positive_issue(
                pull_request.get("number"), "implementation pull request number"
            )
            if should_have_label:
                gh_api_json(
                    [
                        f"repos/{owner}/{name}/issues/{number}/labels",
                        "--method",
                        "POST",
                        "-H",
                        f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
                        "-f",
                        f"labels[]={ISSUEOPS_ADMISSION_LABEL}",
                    ]
                )
            else:
                gh_api_json(
                    [
                        f"repos/{owner}/{name}/issues/{number}/labels/"
                        f"{quote(ISSUEOPS_ADMISSION_LABEL, safe='')}",
                        "--method",
                        "DELETE",
                        "-H",
                        f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
                    ]
                )
    except BaseException as error:
        detail = str(error).strip() or error.__class__.__name__
        return ImplementationAdmission(
            admitted=admitted,
            overflow=overflow,
            evicted=tuple(evicted),
            failures=(f"IssueOps admission label reconciliation failed: {detail}",),
            ready=False,
        )
    return ImplementationAdmission(
        admitted=admitted,
        overflow=overflow,
        evicted=tuple(evicted),
        failures=(),
        ready=True,
    )


def implementation_pr_requires_admission(
    repo: str,
    pull_request: dict[str, object],
    release_graphs: dict[str, ReleaseGraph],
) -> tuple[bool, str | None]:
    """Check a PR-side gate against the maintained deterministic admission set."""

    references = pull_request.get("closingIssuesReferences")
    if not isinstance(references, list):
        return False, "implementation PR has malformed closing references"
    graph_issue_numbers = {
        issue for graph in release_graphs.values() for issue in graph.blocked_by
    }
    try:
        reference_numbers = {
            native_relation_issue_number(repo, reference, "closing issue reference")
            for reference in references
        }
    except BaseException as error:
        return False, str(error)
    if not reference_numbers.intersection(graph_issue_numbers):
        return True, None
    pull_requests = open_pull_requests_snapshot(repo)
    implementation_prs, selection_failures = implementation_prs_for_release_graphs(
        repo, pull_requests, release_graphs
    )
    if selection_failures:
        return False, selection_failures[0]
    number = positive_issue(pull_request.get("number"), "pull request number")
    ordered = sorted(
        implementation_prs,
        key=lambda item: positive_issue(item.get("number"), "implementation pull request number"),
    )
    admitted_numbers = {
        positive_issue(item.get("number"), "implementation pull request number")
        for item in ordered[:MAX_ACTIVE_IMPLEMENTATION_PRS]
    }
    if number not in admitted_numbers:
        return (
            False,
            f"PR #{number} is outside the {MAX_ACTIVE_IMPLEMENTATION_PRS}-PR "
            "IssueOps implementation admission ceiling",
        )
    matching = next((item for item in pull_requests if item.get("number") == number), None)
    if not isinstance(matching, dict) or not pull_request_has_admission_label(matching):
        return False, f"PR #{number} is not marked with the IssueOps admission label"
    return True, None


def affected_implementation_prs(
    repo: str,
    issue: int,
    pull_requests: tuple[dict[str, object], ...],
    graphs: dict[str, ReleaseGraph],
    additional_issues: tuple[int, ...] = (),
) -> tuple[list[dict[str, object]], list[str]]:
    """Select affected PRs while isolating malformed entries as failures."""

    impacted = {positive_issue(issue, "issue number")}
    impacted.update(
        positive_issue(candidate, "additional issue number")
        for candidate in additional_issues
    )
    changed = True
    while changed:
        changed = False
        for graph in graphs.values():
            for candidate, blockers in graph.blocked_by.items():
                if candidate not in impacted and impacted.intersection(blockers):
                    impacted.add(candidate)
                    changed = True
    affected: list[dict[str, object]] = []
    failures: list[str] = []
    for pull_request in pull_requests:
        number = pull_request.get("number", "unknown")
        try:
            references = pull_request.get("closingIssuesReferences")
            if not isinstance(references, list):
                raise SystemExit("malformed closing references")
            if any(
                native_relation_issue_number(repo, reference, "closing issue reference")
                in impacted
                for reference in references
            ):
                affected.append(pull_request)
        except BaseException as error:
            failures.append(f"PR #{number}: {error}")
    return affected, failures


def build_issueops_snapshot(
    repo: str,
    issue: int,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    additional_issues: tuple[int, ...] = (),
    reconcile_all_release_graphs: bool = False,
    enforce_admission: bool = False,
) -> tuple[IssueOpsSnapshot, list[dict[str, object]]]:
    """Build one bounded immutable issue/graph/PR snapshot for a trigger."""

    pull_requests = open_pull_requests_snapshot(repo)
    selection_issues = additional_issues
    if reconcile_all_release_graphs:
        selection_issues = tuple(
            sorted(
                {
                    candidate
                    for graph in release_graphs.values()
                    for candidate in graph.blocked_by
                }
            )
        )
    affected, selection_failures = affected_implementation_prs(
        repo, issue, pull_requests, release_graphs, selection_issues
    )
    admission = ImplementationAdmission(
        admitted=tuple(affected),
        overflow=(),
        evicted=(),
        failures=(),
        ready=True,
    )
    if enforce_admission:
        admission = reconcile_admission_labels(repo, affected)
        affected = list(admission.admitted)
        selection_failures.extend(admission.failures)
        if admission.overflow:
            selection_failures.append(
                f"{len(admission.overflow)} implementation PR(s) exceed the "
                f"{MAX_ACTIVE_IMPLEMENTATION_PRS}-PR admission ceiling: "
                + ", ".join(
                    f"#{positive_issue(item.get('number'), 'implementation pull request number')}"
                    for item in admission.overflow
                )
            )
    trigger_issues = (issue, *selection_issues)
    graph_milestones = {
        graph.milestone
        for graph in release_graphs.values()
        if any(
            positive_issue(candidate, "trigger issue number") in graph.blocked_by
            for candidate in trigger_issues
        )
    }
    for pull_request in affected:
        references = pull_request.get("closingIssuesReferences", [])
        if isinstance(references, list):
            for reference in references:
                try:
                    reference_number = native_relation_issue_number(
                        repo, reference, "closing issue reference"
                    )
                except BaseException:
                    continue
                graph = graph_for_issue(release_graphs, reference_number)
                if graph is not None:
                    graph_milestones.add(graph.milestone)
    relevant_graphs = {
        milestone: graph
        for milestone, graph in release_graphs.items()
        if milestone in graph_milestones
    }
    graph_budget = graph_request_budget(relevant_graphs)
    if graph_budget > MAX_GRAPH_REQUESTS_PER_EVENT:
        selection_failures.append(
            f"IssueOps graph request budget {graph_budget} exceeds the bounded "
            f"limit {MAX_GRAPH_REQUESTS_PER_EVENT}"
        )
    issue_numbers = {positive_issue(issue, "issue number")}
    for graph in relevant_graphs.values():
        issue_numbers.update(graph.blocked_by)
    for pull_request in affected:
        references = pull_request.get("closingIssuesReferences", [])
        if isinstance(references, list):
            for reference in references:
                try:
                    issue_numbers.add(
                        native_relation_issue_number(repo, reference, "closing issue reference")
                    )
                except BaseException:
                    pass
    issue_payloads = {}
    graph_failures: dict[str, tuple[str, ...]] = {}
    if graph_budget <= MAX_GRAPH_REQUESTS_PER_EVENT:
        issue_payloads = {
            number: issue_payload(repo, number) for number in sorted(issue_numbers)
        }
        graph_failures = {
            milestone: tuple(
                release_graph_failures(repo, relevant_graphs, issue_map, {milestone})
            )
            for milestone in sorted(relevant_graphs)
        }
    snapshot = IssueOpsSnapshot(
        pull_requests=pull_requests,
        issue_payloads=issue_payloads,
        graph_failures=graph_failures,
        selection_failures=tuple(selection_failures),
        overflow_implementation_prs=admission.overflow,
        evicted_implementation_prs=admission.evicted,
        admission_ready=admission.ready and graph_budget <= MAX_GRAPH_REQUESTS_PER_EVENT,
    )
    return snapshot, affected


def implementation_prs_for_issue(
    repo: str,
    issue: int,
    pull_requests: object,
    graphs: dict[str, ReleaseGraph] | None = None,
) -> list[dict[str, object]]:
    """Select open implementation PRs affected by one changed release issue."""

    issue = positive_issue(issue, "issue number")
    impacted = {issue}
    if graphs:
        changed = True
        while changed:
            changed = False
            for graph in graphs.values():
                for candidate, blockers in graph.blocked_by.items():
                    if candidate not in impacted and impacted.intersection(blockers):
                        impacted.add(candidate)
                        changed = True
    if not isinstance(pull_requests, list):
        raise SystemExit("GitHub open pull-request response must be an array")
    affected: list[dict[str, object]] = []
    for pull_request in pull_requests:
        if not isinstance(pull_request, dict):
            raise SystemExit("GitHub open pull-request response contained a non-object")
        references = pull_request.get("closingIssuesReferences")
        if not isinstance(references, list):
            raise SystemExit(
                "GitHub open pull-request response contained malformed closing references"
            )
        if any(
            native_relation_issue_number(repo, reference, "closing issue reference") in impacted
            for reference in references
        ):
            affected.append(pull_request)
    return affected


def commit_status(
    repo: str, sha: str, state: str, description: str
) -> None:
    """Publish one fail-closed GitHub commit status for an exact PR head."""

    if re.fullmatch(r"[0-9a-fA-F]{40,64}", sha) is None:
        raise SystemExit("GitHub pull request head is not an exact commit SHA")
    if state not in {"pending", "success", "failure", "error"}:
        raise SystemExit(f"invalid implementation status state: {state!r}")
    if not description or len(description) > 140:
        raise SystemExit("implementation status description must be 1..140 characters")
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
            f"context={IMPLEMENTATION_STATUS_CONTEXT}",
            "-f",
            f"description={description}",
        ]
    )
    if not isinstance(payload, dict):
        raise SystemExit("GitHub commit status response must be an object")


def implementation_reference_failures(
    repo: str,
    reference: object,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    snapshot: IssueOpsSnapshot | None = None,
) -> list[str]:
    """Run the default-branch implementation checker for one native reference."""

    number = native_relation_issue_number(repo, reference, "closing issue reference")
    issue = (
        snapshot.issue_payloads[number]
        if snapshot is not None and number in snapshot.issue_payloads
        else issue_payload(repo, number)
    )
    target_graph, failures = target_graph_failures(issue, release_graphs)
    failures.extend(
        check_openspec_tasks(
            repo,
            root,
            issue_map,
            planned_issue=number,
            issue_payloads=snapshot.issue_payloads if snapshot is not None else None,
        )
    )
    failures.extend(planned_issue_failures(issue, issue_map, root))
    failures.extend(
        implementation_issue_failures(
            repo,
            issue,
            release_graphs,
            issue_payloads=snapshot.issue_payloads if snapshot is not None else None,
        )
    )
    graph_milestones: set[str] = set()
    if target_graph is not None:
        graph_milestones.add(target_graph.milestone)
    else:
        milestone = issue.get("milestone")
        if isinstance(milestone, dict) and isinstance(milestone.get("title"), str):
            graph_milestones.add(milestone["title"])
    if snapshot is not None:
        for milestone in sorted(graph_milestones):
            failures.extend(snapshot.graph_failures.get(milestone, ()))
    else:
        failures.extend(
            release_graph_failures(
                repo,
                release_graphs,
                issue_map,
                graph_milestones or None,
            )
        )
    return failures


def prepare_implementation_status_candidates(
    pull_requests: list[dict[str, object]],
) -> tuple[list[ImplementationStatusCandidate], list[str]]:
    """Validate every selected PR head before publishing any pending status."""

    candidates: list[ImplementationStatusCandidate] = []
    failures: list[str] = []
    seen: set[int] = set()
    for pull_request in pull_requests:
        number_value = pull_request.get("number", "unknown")
        try:
            number = positive_issue(number_value, "pull request number")
            if number in seen:
                raise SystemExit(f"duplicate selected pull request #{number}")
            seen.add(number)
            expected_sha = pull_request.get("headRefOid")
            if not isinstance(expected_sha, str) or re.fullmatch(
                r"[0-9a-fA-F]{40,64}", expected_sha
            ) is None:
                raise SystemExit(
                    f"GitHub pull request #{number} is missing an exact head commit"
                )
            candidates.append(
                ImplementationStatusCandidate(pull_request, number, expected_sha)
            )
        except SystemExit as error:
            failures.append(f"PR #{number_value}: {error}")
    return candidates, failures


def verify_live_candidate_head(
    repo: str, candidate: ImplementationStatusCandidate
) -> None:
    """Bind one candidate to its live head before a status publication."""

    live_pull_request = gh_json(
        [
            "pr",
            "view",
            str(candidate.number),
            "-R",
            repo,
            "--json",
            "number,headRefOid",
        ]
    )
    if not isinstance(live_pull_request, dict):
        raise SystemExit(
            f"GitHub pull request #{candidate.number} head read-back was not an object"
        )
    live_number = positive_issue(
        live_pull_request.get("number"), "live pull request number"
    )
    live_sha = live_pull_request.get("headRefOid")
    if live_number == candidate.number and isinstance(live_sha, str):
        if live_sha != candidate.expected_sha:
            raise CandidateHeadChanged(candidate.number, candidate.expected_sha, live_sha)
    elif live_number != candidate.number or live_sha != candidate.expected_sha:
        raise SystemExit(
            f"PR #{candidate.number} head changed before status publication: "
            f"expected {candidate.expected_sha}, found {live_sha or 'missing'}"
        )


def run_bounded_status_work(
    candidates: list[ImplementationStatusCandidate],
    worker: Callable[[ImplementationStatusCandidate], None],
    batch_precondition: Callable[[list[ImplementationStatusCandidate]], str | None]
    | None = None,
) -> list[str]:
    """Drain bounded status work while retaining every candidate failure."""

    failures: list[str] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=STATUS_WORKERS) as executor:
        for offset in range(0, len(candidates), STATUS_WORKERS):
            batch = candidates[offset : offset + STATUS_WORKERS]
            if batch_precondition is not None:
                try:
                    precondition_failure = batch_precondition(batch)
                except BaseException as error:
                    detail = str(error).strip() or error.__class__.__name__
                    failures.append(
                        f"PR #{batch[0].number}: status batch precondition failed: {detail}"
                    )
                    break
                if precondition_failure is not None:
                    failures.append(precondition_failure)
                    break
            futures = [executor.submit(worker, candidate) for candidate in batch]
            for candidate, future in zip(batch, futures):
                try:
                    future.result()
                except BaseException as error:
                    detail = str(error).strip() or error.__class__.__name__
                    failures.append(f"PR #{candidate.number}: {detail}")
    return failures


def publish_pending_statuses(
    repo: str, candidates: list[ImplementationStatusCandidate]
) -> list[str]:
    """Establish pending status for every candidate before expensive evaluation."""

    validated: set[int] = set()
    raced_heads: dict[int, str] = {}
    validation_lock = threading.Lock()

    def validate(candidate: ImplementationStatusCandidate) -> None:
        try:
            verify_live_candidate_head(repo, candidate)
        except CandidateHeadChanged as error:
            with validation_lock:
                raced_heads[candidate.number] = error.live_sha
            raise
        with validation_lock:
            validated.add(candidate.number)

    head_failures = run_bounded_status_work(
        candidates, validate
    )
    stable_candidates = [candidate for candidate in candidates if candidate.number in validated]
    raced_candidates = [candidate for candidate in candidates if candidate.number in raced_heads]
    candidates[:] = stable_candidates
    raced_failures = run_bounded_status_work(
        raced_candidates,
        lambda candidate: commit_status(
            repo,
            raced_heads[candidate.number],
            "failure",
            "PR head changed before IssueOps revalidation completed",
        ),
    )
    pending_candidates: set[int] = set()
    pending_lock = threading.Lock()

    def publish_pending(candidate: ImplementationStatusCandidate) -> None:
        commit_status(
            repo,
            candidate.expected_sha,
            "pending",
            "Revalidating native implementation references",
        )
        with pending_lock:
            pending_candidates.add(candidate.number)

    pending_failures = run_bounded_status_work(stable_candidates, publish_pending)
    candidates[:] = [
        candidate for candidate in stable_candidates if candidate.number in pending_candidates
    ]
    return head_failures + raced_failures + pending_failures


def publish_admission_failure_statuses(
    repo: str, pull_requests: tuple[dict[str, object], ...]
) -> list[str]:
    """Fail PRs evicted from an admission slot without claiming a stale head."""

    def fail_one(pull_request: dict[str, object]) -> None:
        number = positive_issue(pull_request.get("number"), "implementation pull request number")
        live = gh_json(
            [
                "pr",
                "view",
                str(number),
                "-R",
                repo,
                "--json",
                "number,headRefOid",
            ]
        )
        if not isinstance(live, dict):
            raise SystemExit(f"PR #{number} head read-back was not an object")
        live_number = positive_issue(live.get("number"), "live pull request number")
        live_sha = live.get("headRefOid")
        if live_number != number or not isinstance(live_sha, str):
            raise SystemExit(f"PR #{number} admission failure head read-back was invalid")
        commit_status(
            repo,
            live_sha,
            "failure",
            "IssueOps implementation admission slot is no longer available",
        )

    return run_bounded_status_work(
        [
            ImplementationStatusCandidate(
                pull_request,
                positive_issue(pull_request.get("number"), "implementation pull request number"),
                pull_request.get("headRefOid", ""),
            )
            for pull_request in pull_requests
        ],
        lambda candidate: fail_one(candidate.pull_request),
    )


def finalize_implementation_statuses(
    repo: str,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    snapshot: IssueOpsSnapshot,
    candidates: list[ImplementationStatusCandidate],
    run_id: int | None = None,
) -> list[str]:
    """Evaluate all candidates after mutation, retaining pending/failure on errors."""

    def generation_precondition(
        batch: list[ImplementationStatusCandidate],
    ) -> str | None:
        if run_id is None:
            return None
        try:
            if newer_issueops_run_exists(repo, run_id):
                return (
                    f"IssueOps workflow run {run_id} was superseded before final status "
                    f"batch starting at PR #{batch[0].number}"
                )
        except BaseException as error:
            detail = str(error).strip() or error.__class__.__name__
            return (
                f"PR #{batch[0].number}: IssueOps generation check failed before final "
                f"status batch: {detail}"
            )
        return None

    def finalize(candidate: ImplementationStatusCandidate) -> None:
        publish_implementation_status_for_pr(
            repo,
            candidate.number,
            root,
            issue_map,
            release_graphs,
            candidate.pull_request,
            expected_pr_head_sha=candidate.expected_sha,
            snapshot=snapshot,
            skip_pending=True,
            run_id=run_id,
            generation_checked=run_id is not None,
        )

    return run_bounded_status_work(
        candidates,
        finalize,
        batch_precondition=generation_precondition if run_id is not None else None,
    )


def refresh_snapshot_graph_failures(
    repo: str,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    snapshot: IssueOpsSnapshot,
) -> IssueOpsSnapshot:
    """Refresh live issue payloads and each preselected graph once before finalization."""

    refreshed_issue_payloads = {
        number: issue_payload(repo, number) for number in sorted(snapshot.issue_payloads)
    }
    refreshed = {
        milestone: tuple(
            release_graph_failures(repo, release_graphs, issue_map, {milestone})
        )
        for milestone in sorted(snapshot.graph_failures)
    }
    return IssueOpsSnapshot(
        pull_requests=snapshot.pull_requests,
        issue_payloads=refreshed_issue_payloads,
        graph_failures=refreshed,
        selection_failures=snapshot.selection_failures,
        overflow_implementation_prs=snapshot.overflow_implementation_prs,
        evicted_implementation_prs=snapshot.evicted_implementation_prs,
        admission_ready=snapshot.admission_ready,
    )


def publish_implementation_status_for_pr(
    repo: str,
    pull_request_number: int,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    pull_request: dict[str, object] | None = None,
    expected_pr_head_sha: str | None = None,
    snapshot: IssueOpsSnapshot | None = None,
    skip_pending: bool = False,
    run_id: int | None = None,
    generation_checked: bool = False,
) -> None:
    """Validate live native references and publish pending then final status."""

    loaded_pull_request = pull_request is None
    if pull_request is None:
        pull_request = gh_json(
            [
                "pr",
                "view",
                str(positive_issue(pull_request_number, "pull request number")),
                "-R",
                repo,
                "--json",
                "number,headRefOid,closingIssuesReferences,author,labels",
            ]
        )
    if not isinstance(pull_request, dict):
        raise SystemExit("GitHub pull-request response must be an object")
    number = positive_issue(pull_request.get("number"), "pull request number")
    if number != positive_issue(pull_request_number, "pull request number"):
        raise SystemExit("GitHub pull-request response number did not match the request")
    sha = pull_request.get("headRefOid")
    if not isinstance(sha, str):
        raise SystemExit(f"GitHub pull request #{number} is missing its head commit")
    expected_sha = expected_pr_head_sha or sha
    if sha != expected_sha:
        raise SystemExit(
            f"PR #{number} head changed before status publication: expected {expected_sha}, "
            f"found {sha}"
        )
    references = pull_request.get("closingIssuesReferences")
    if not isinstance(references, list):
        raise SystemExit(
            f"GitHub pull request #{number} has malformed closing issue references"
        )
    if not skip_pending:
        if loaded_pull_request and snapshot is None:
            try:
                admitted, admission_failure = implementation_pr_requires_admission(
                    repo, pull_request, release_graphs
                )
            except BaseException as error:
                admitted = False
                admission_failure = str(error).strip() or error.__class__.__name__
            if not admitted:
                detail = admission_failure or "PR is outside the IssueOps admission set"
                try:
                    commit_status(
                        repo,
                        sha,
                        "failure",
                        "IssueOps implementation admission failed: " + clean(detail),
                    )
                except SystemExit as error:
                    raise SystemExit(
                        f"unable to publish {IMPLEMENTATION_STATUS_CONTEXT} admission failure "
                        f"for PR #{number}: {error}"
                    ) from error
                raise SystemExit(f"PR #{number} was not admitted: {detail}")
        commit_status(repo, sha, "pending", "Revalidating native implementation references")
    failures: list[str] = []
    if snapshot is not None and not snapshot.admission_ready:
        failures.append("IssueOps admission or graph request budget was not established")
    author = pull_request.get("author")
    author_login = author.get("login") if isinstance(author, dict) else None
    if author_login == "dependabot[bot]":
        description = "Dependabot dependency update uses the standard CI path"
    elif not references:
        description = "Planning PR has no native implementation closing reference"
    else:
        for reference in references:
            try:
                failures.extend(
                    implementation_reference_failures(
                        repo,
                        reference,
                        root,
                        issue_map,
                        release_graphs,
                        snapshot,
                    )
                )
            except SystemExit as error:
                failures.append(str(error))
            except Exception as error:
                failures.append(f"implementation validation failed: {error}")
        description = (
            "Native implementation references passed"
            if not failures
            else "Native implementation references failed: " + clean(failures[0])
        )[:140]
    live_pull_request = gh_json(
        [
            "pr",
            "view",
            str(number),
            "-R",
            repo,
            "--json",
            "number,headRefOid",
        ]
    )
    if not isinstance(live_pull_request, dict):
        raise SystemExit(f"GitHub pull request #{number} head read-back was not an object")
    live_sha = live_pull_request.get("headRefOid")
    if live_sha != expected_sha:
        raise SystemExit(
            f"PR #{number} head changed before status publication: expected {expected_sha}, "
            f"found {live_sha or 'missing'}"
        )
    state = "success" if not failures else "failure"
    if (
        state == "success"
        and run_id is not None
        and not generation_checked
        and newer_issueops_run_exists(repo, run_id)
    ):
        raise SystemExit(
            f"IssueOps workflow run {run_id} was superseded before PR #{number} success publication"
        )
    try:
        commit_status(repo, expected_sha, state, description)
    except SystemExit as error:
        raise SystemExit(
            f"unable to publish {IMPLEMENTATION_STATUS_CONTEXT} for PR #{number}: {error}"
        ) from error
    if failures:
        raise SystemExit(
            f"{IMPLEMENTATION_STATUS_CONTEXT} failed for PR #{number}:\n"
            + "\n".join(f"- {failure}" for failure in failures)
        )


def publish_implementation_statuses_for_issue(
    repo: str,
    issue: int,
    root: Path,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    additional_issues: tuple[int, ...] = (),
    reconcile_all_release_graphs: bool = False,
    run_id: int | None = None,
    enforce_admission: bool = False,
) -> None:
    """Refresh selected implementation PRs from one bounded live snapshot."""

    snapshot, affected = build_issueops_snapshot(
        repo,
        issue,
        issue_map,
        release_graphs,
        additional_issues,
        reconcile_all_release_graphs,
        enforce_admission,
    )
    candidates, candidate_failures = prepare_implementation_status_candidates(affected)
    failures: list[str] = list(snapshot.selection_failures)
    failures.extend(candidate_failures)
    failures.extend(
        publish_admission_failure_statuses(
            repo, snapshot.evicted_implementation_prs
        )
    )
    pending_failures = publish_pending_statuses(repo, candidates)
    failures.extend(pending_failures)
    final_snapshot = refresh_snapshot_graph_failures(
        repo, issue_map, release_graphs, snapshot
    )
    failures.extend(
        finalize_implementation_statuses(
            repo,
            root,
            issue_map,
            release_graphs,
            final_snapshot,
            candidates,
            run_id=run_id,
        )
    )
    if failures:
        raise SystemExit("affected implementation status publication failed:\n" + "\n".join(failures))


def invalidate_implementation_statuses_for_issue(
    repo: str,
    issue: int,
    issue_map: dict[str, tuple[Owner, ...]],
    release_graphs: dict[str, ReleaseGraph],
    reconcile_all_release_graphs: bool = False,
    enforce_admission: bool = False,
) -> None:
    """Publish fail-closed pending statuses before a queued finalizer runs."""

    snapshot, affected = build_issueops_snapshot(
        repo,
        issue,
        issue_map,
        release_graphs,
        reconcile_all_release_graphs=reconcile_all_release_graphs,
        enforce_admission=enforce_admission,
    )
    candidates, candidate_failures = prepare_implementation_status_candidates(affected)
    failures = list(snapshot.selection_failures) + candidate_failures
    failures.extend(
        publish_admission_failure_statuses(
            repo, snapshot.evicted_implementation_prs
        )
    )
    failures.extend(publish_pending_statuses(repo, candidates))
    if failures:
        raise SystemExit("IssueOps invalidation failed:\n" + "\n".join(failures))


def enforce_closed_issue_blockers(
    repo: str, issue_number: int, graphs: dict[str, ReleaseGraph]
) -> list[str]:
    """Reopen any declared issue that closes while a direct blocker remains open."""

    issue = issue_payload(repo, positive_issue(issue_number, "issue number"))
    if str(issue.get("state", "")).upper() != "CLOSED":
        return []
    failures = implementation_issue_failures(repo, issue, graphs)
    if not failures:
        return []
    run(["gh", "issue", "reopen", str(issue_number), "--repo", repo])
    raise SystemExit(
        "closed issue was reopened because blocker enforcement failed:\n"
        + "\n".join(f"- {failure}" for failure in failures)
    )


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


def implementation_issue_failures(
    repo: str,
    issue: dict[str, object],
    graphs: dict[str, ReleaseGraph],
    issue_payloads: dict[int, dict[str, object]] | None = None,
) -> list[str]:
    """Require every declared direct blocker of an issue to be closed."""

    number = positive_issue(issue.get("number"), "issue number")
    graph = graph_for_issue(graphs, number)
    if graph is None:
        return []
    failures: list[str] = []
    for blocker in graph.blocked_by[number]:
        try:
            blocker_issue = (
                issue_payloads[blocker]
                if issue_payloads is not None and blocker in issue_payloads
                else issue_payload(repo, blocker)
            )
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
    assert flatten_paginated_response([[{"number": 1}], [{"number": 2}]]) == [
        {"number": 1},
        {"number": 2},
    ]
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
    saved_native_issue_id = globals()["native_issue_id"]
    saved_issue_payload = globals()["issue_payload"]
    saved_gh_json = globals()["gh_json"]
    saved_run = globals()["run"]
    saved_gh_api_json = globals()["gh_api_json"]
    saved_subprocess_run = subprocess.run
    try:
        api_args: list[list[str]] = []
        local_identity = {"repository_url": "https://api.github.com/repos/owner/repo"}

        def fake_gh_api_json(args: list[str]) -> object:
            api_args.append(args)
            joined = " ".join(args)
            if "/milestones" in joined:
                return [[{"title": "v1.2.3-00", "number": 7}]]
            if "/dependencies/blocked_by" in joined:
                return [[{**local_identity, "number": 11}]]
            if "milestone=7" in joined:
                return [[{"number": 10}, {"number": 11}]]
            return [[
                {**local_identity, "number": 10},
                {**local_identity, "number": 11},
            ]]

        globals()["gh_api_json"] = fake_gh_api_json
        assert native_blocked_by("owner/repo", 10) == {11}
        assert native_sub_issues("owner/repo", 12) == {10, 11}
        local_closing_reference = {
            "number": 11,
            "repository": {"name": "repo", "owner": {"login": "owner"}},
        }
        assert (
            native_relation_issue_number(
                "owner/repo", local_closing_reference, "closing issue reference"
            )
            == 11
        )

        class FakeProcess:
            def __init__(self, payload: object) -> None:
                self.returncode = 0
                self.stdout = "HTTP/2.0 200 OK\r\n\r\n" + json.dumps(payload)
                self.stderr = ""

        subprocess.run = lambda *args, **kwargs: FakeProcess(
            {**local_identity, "number": 12}
        )
        assert native_parent_issue("owner/repo", 10) == 12

        foreign_identity = {
            "repository_url": "https://api.github.com/repos/foreign/repo",
            "number": 11,
        }
        foreign_closing_reference = {
            "number": 11,
            "repository": {"name": "repo", "owner": {"login": "foreign"}},
        }
        conflicting_identity = {
            **local_identity,
            "repository": {"full_name": "foreign/repo"},
            "number": 11,
        }
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
        try:
            native_relation_issue_number(
                "owner/repo", conflicting_identity, "conflicting native relation"
            )
        except SystemExit as error:
            assert "exact owner/repo repository identity" in str(error)
        else:
            raise AssertionError("conflicting native identities were accepted")
        try:
            native_relation_issue_number(
                "owner/repo", foreign_closing_reference, "closing issue reference"
            )
        except SystemExit as error:
            assert "exact owner/repo repository identity" in str(error)
        else:
            raise AssertionError("foreign closing issue reference was accepted")
        for malformed in (
            {"number": 11},
            {"number": 11, "repository_url": 12},
            {"number": 11, "repository": {"full_name": 12}},
            {
                "number": 11,
                "repository": {"name": "repo", "owner": {"login": 12}},
            },
        ):
            try:
                native_relation_issue_number("owner/repo", malformed, "native relation")
            except SystemExit as error:
                assert "repository identity" in str(error)
            else:
                raise AssertionError("missing or malformed native identity was accepted")

        globals()["gh_api_json"] = lambda args: [[foreign_identity]]
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
        implementation_pr = {
            "number": 494,
            "headRefOid": "a" * 40,
            "closingIssuesReferences": [
                {
                    "number": 10,
                    "repository": {"name": "repo", "owner": {"login": "owner"}},
                }
            ],
        }
        assert implementation_prs_for_issue("owner/repo", 10, [implementation_pr]) == [
            implementation_pr
        ]
        dependent_graphs = parse_release_graphs(
            {
                "schema_version": 2,
                "changes": {},
                "release_graphs": {
                    "v1.2.3-00": {
                        "release_issue": 12,
                        "issues": {"10": [], "11": [10], "12": [10, 11]},
                    }
                },
            },
            Path("issue-map.json"),
            graph_owners,
        )
        assert implementation_prs_for_issue(
            "owner/repo", 10, [
                {
                    **implementation_pr,
                    "closingIssuesReferences": [
                        {
                            "number": 11,
                            "repository": {
                                "name": "repo",
                                "owner": {"login": "owner"},
                            },
                        }
                    ],
                }
            ], dependent_graphs
        )
        status_issue_calls: list[int] = []
        saved_status_publisher = publish_implementation_status_for_pr
        saved_commit_status = commit_status
        globals()["publish_implementation_status_for_pr"] = (
            lambda repo, number, root, issue_map, release_graphs, pull_request=None,
            expected_pr_head_sha=None, snapshot=None, skip_pending=False, run_id=None,
            generation_checked=False: status_issue_calls.append(
                number
            )
        )
        globals()["commit_status"] = lambda *args, **kwargs: None
        globals()["gh_json"] = lambda args: (
            [implementation_pr]
            if args[:2] == ["pr", "list"]
            else {"number": 494, "headRefOid": implementation_pr["headRefOid"]}
        )
        publish_implementation_statuses_for_issue(
            "owner/repo", 10, Path("."), {}, dependent_graphs
        )
        assert status_issue_calls == [494]
        globals()["publish_implementation_status_for_pr"] = saved_status_publisher
        globals()["commit_status"] = saved_commit_status
        try:
            implementation_prs_for_issue(
                "owner/repo",
                10,
                [
                    {
                        **implementation_pr,
                        "closingIssuesReferences": [foreign_closing_reference],
                    }
                ],
            )
        except SystemExit as error:
            assert "exact owner/repo repository identity" in str(error)
        else:
            raise AssertionError("foreign implementation PR reference was accepted")
        assert implementation_prs_for_issue("owner/repo", 10, []) == []
        status_calls: list[list[str]] = []

        def fake_status_gh_json(args: list[str]) -> object:
            assert args[:2] == ["pr", "view"]
            return implementation_pr

        def fake_status_api(args: list[str]) -> object:
            status_calls.append(args)
            return {}

        globals()["gh_json"] = fake_status_gh_json
        globals()["gh_api_json"] = fake_status_api
        publish_implementation_status_for_pr(
            "owner/repo", 494, Path("."), {}, {}
        )
        status_states = [
            argument.split("=", 1)[1]
            for call in status_calls
            for argument in call
            if argument.startswith("state=")
        ]
        assert status_states == ["pending", "success"]
        assert all(
            IMPLEMENTATION_STATUS_CONTEXT in call for call in (" ".join(args) for args in status_calls)
        )

        saved_generation_guard = newer_issueops_run_exists
        globals()["newer_issueops_run_exists"] = lambda repo, run_id: True
        status_calls.clear()
        globals()["gh_json"] = fake_status_gh_json
        try:
            publish_implementation_status_for_pr(
                "owner/repo", 494, Path("."), {}, {}, run_id=100
            )
        except SystemExit as error:
            assert "superseded before PR #494 success publication" in str(error)
        else:
            raise AssertionError("superseded IssueOps run published success")
        status_states = [
            argument.split("=", 1)[1]
            for call in status_calls
            for argument in call
            if argument.startswith("state=")
        ]
        assert status_states == ["pending"]
        globals()["newer_issueops_run_exists"] = saved_generation_guard

        saved_workflow_runs_api = globals()["gh_api_json"]
        globals()["gh_api_json"] = lambda args: {
            "workflow_runs": [
                {
                    "id": 101,
                    "event": "repository_dispatch",
                    "path": ISSUEOPS_WORKFLOW_PATH,
                },
                {"id": 100, "event": "issues", "path": ISSUEOPS_WORKFLOW_PATH},
            ]
        }
        assert newer_issueops_run_exists("owner/repo", 100)
        globals()["gh_api_json"] = lambda args: {
            "workflow_runs": [
                {"id": 100, "event": "issues", "path": ISSUEOPS_WORKFLOW_PATH}
            ]
        }
        assert not newer_issueops_run_exists("owner/repo", 100)
        globals()["gh_api_json"] = saved_workflow_runs_api

        invalidation_pending: list[list[int]] = []
        invalidation_saved = {
            name: globals()[name]
            for name in (
                "build_issueops_snapshot",
                "prepare_implementation_status_candidates",
                "publish_pending_statuses",
            )
        }
        invalidation_candidate = ImplementationStatusCandidate(
            {"number": 912, "headRefOid": "e" * 40}, 912, "e" * 40
        )
        globals()["build_issueops_snapshot"] = lambda *args, **kwargs: (
            IssueOpsSnapshot((), {}, {}, ()),
            [{"number": 912, "headRefOid": "e" * 40}],
        )
        globals()["prepare_implementation_status_candidates"] = (
            lambda affected: ([invalidation_candidate], [])
        )
        globals()["publish_pending_statuses"] = lambda repo, candidates: (
            invalidation_pending.append([candidate.number for candidate in candidates])
            or []
        )
        invalidate_implementation_statuses_for_issue(
            "owner/repo", 314, {}, {}, reconcile_all_release_graphs=True
        )
        assert invalidation_pending == [[912]]
        for name, value in invalidation_saved.items():
            globals()[name] = value

        status_calls.clear()
        changed_heads = iter(
            [implementation_pr, {**implementation_pr, "headRefOid": "b" * 40}]
        )
        globals()["gh_json"] = lambda args: next(changed_heads)
        try:
            publish_implementation_status_for_pr(
                "owner/repo",
                494,
                Path("."),
                {},
                {},
                expected_pr_head_sha="a" * 40,
            )
        except SystemExit as error:
            assert "head changed before status publication" in str(error)
        else:
            raise AssertionError("changed PR head was accepted")
        status_states = [
            argument.split("=", 1)[1]
            for call in status_calls
            for argument in call
            if argument.startswith("state=")
        ]
        assert status_states == ["pending"]

        mutation_calls: list[list[str]] = []
        blocked_edges: set[int] = set()
        sub_issue_edges: set[int] = set()
        globals()["native_blocked_by"] = lambda repo, issue: set(blocked_edges)
        globals()["native_sub_issues"] = lambda repo, issue: set(sub_issue_edges)
        globals()["native_parent_issue"] = lambda repo, issue: None
        globals()["native_issue_id"] = lambda repo, issue, label: issue + 1000

        def fake_mutation_run(args: list[str]) -> str:
            mutation_calls.append(args)
            if "POST" in args:
                if "blocked_by" in args[2]:
                    blocked_edges.add(11)
                else:
                    sub_issue_edges.add(11)
            elif "DELETE" in args:
                if "blocked_by" in args[2]:
                    blocked_edges.discard(11)
                else:
                    sub_issue_edges.discard(11)
            return "{}"

        globals()["run"] = fake_mutation_run
        mutation_results = [
            mutate_native_relationship("owner/repo", 10, 11, "blocked_by", "add"),
            mutate_native_relationship("owner/repo", 10, 11, "blocked_by", "add"),
            mutate_native_relationship("owner/repo", 10, 11, "blocked_by", "remove"),
            mutate_native_relationship("owner/repo", 10, 11, "sub_issue", "add"),
            mutate_native_relationship("owner/repo", 10, 11, "sub_issue", "remove"),
        ]
        assert mutation_results == [True, False, True, True, True]
        assert sum("POST" in call for call in mutation_calls) == 2
        assert sum("DELETE" in call for call in mutation_calls) == 2
        assert mutation_calls == [
            [
                "gh",
                "api",
                "repos/owner/repo/issues/10/dependencies/blocked_by",
                "--method",
                "POST",
                "-H",
                f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
                "-F",
                "issue_id=1011",
            ],
            [
                "gh",
                "api",
                "repos/owner/repo/issues/10/dependencies/blocked_by/1011",
                "--method",
                "DELETE",
                "-H",
                f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
            ],
            [
                "gh",
                "api",
                "repos/owner/repo/issues/10/sub_issues",
                "--method",
                "POST",
                "-H",
                f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
                "-F",
                "sub_issue_id=1011",
            ],
            [
                "gh",
                "api",
                "repos/owner/repo/issues/10/sub_issue",
                "--method",
                "DELETE",
                "-H",
                f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
                "-F",
                "sub_issue_id=1011",
            ],
        ]
        assert all(
            any(GITHUB_API_VERSION in argument for argument in call)
            for call in mutation_calls
        )
        request_id = validate_request_id("rel-20260823-001")
        assert json.loads(relationship_outcome(request_id, "applied"))["outcome"] == "applied"
        assert (
            json.loads(relationship_outcome(request_id, "already-satisfied"))["outcome"]
            == "already-satisfied"
        )
        for invalid_request_id in ("", "short", "bad space", "x" * 65, "-leading-8"):
            try:
                validate_request_id(invalid_request_id)
            except SystemExit:
                pass
            else:
                raise AssertionError("invalid relationship request_id was accepted")
        assert validate_native_relationship_request("sub_issue", "add", "492", "339") == (
            492,
            339,
        )
        for malformed_relationship in (
            ("none", "add", "492", "339"),
            ("sub_issue", "none", "492", "339"),
            ("sub_issue", "remove", None, "339"),
            ("sub_issue", "remove", "492", ""),
        ):
            try:
                validate_native_relationship_request(*malformed_relationship)
            except SystemExit:
                failed = json.loads(relationship_outcome(request_id, "failed"))
                assert failed == {
                    "event": "issueops_relationship",
                    "outcome": "failed",
                    "request_id": request_id,
                }
                assert failed["outcome"] not in {"applied", "already-satisfied"}
            else:
                raise AssertionError(
                    "malformed relationship payload was accepted: "
                    f"{malformed_relationship!r}"
                )
        try:
            mutate_native_relationship("owner/repo", 10, 10, "blocked_by", "add")
        except SystemExit as error:
            assert "itself" in str(error)
        else:
            raise AssertionError("self native relation was accepted")
        mutation_revalidation: list[tuple[str, object]] = []
        saved_mutation = mutate_native_relationship
        saved_snapshot_builder = build_issueops_snapshot
        saved_candidate_builder = prepare_implementation_status_candidates
        saved_pending_publisher = publish_pending_statuses
        saved_status_finalizer = finalize_implementation_statuses
        globals()["mutate_native_relationship"] = (
            lambda repo, issue, related_issue, relation_kind, operation: mutation_revalidation.append(
                ("mutate", (issue, related_issue))
            )
        )
        globals()["build_issueops_snapshot"] = lambda *args, **kwargs: (
            IssueOpsSnapshot((), {}, {}, ()), []
        )
        globals()["prepare_implementation_status_candidates"] = (
            lambda affected: ([], [])
        )
        globals()["publish_pending_statuses"] = (
            lambda repo, candidates: mutation_revalidation.append(("pending", candidates))
        )
        globals()["finalize_implementation_statuses"] = (
            lambda repo, root, issue_map, release_graphs, snapshot, candidates, run_id=None: mutation_revalidation.append(
                ("finalize", candidates)
            )
            or []
        )
        mutate_native_relationship_and_revalidate(
            "owner/repo", 10, 11, "blocked_by", "add", Path("."), {}, {}
        )
        assert [event for event, _ in mutation_revalidation] == [
            "pending",
            "mutate",
            "finalize",
        ]
        globals()["mutate_native_relationship"] = saved_mutation
        globals()["build_issueops_snapshot"] = saved_snapshot_builder
        globals()["prepare_implementation_status_candidates"] = saved_candidate_builder
        globals()["publish_pending_statuses"] = saved_pending_publisher
        globals()["finalize_implementation_statuses"] = saved_status_finalizer

        globals()["gh_json"] = lambda args: [{}] * (MAX_OPEN_PULL_REQUESTS + 1)
        try:
            open_pull_requests_snapshot("owner/repo")
        except SystemExit as error:
            assert "bounded limit" in str(error)
        else:
            raise AssertionError("truncated open-PR snapshot was accepted")
        globals()["gh_json"] = lambda args: [{}] * MAX_OPEN_PULL_REQUESTS
        assert len(open_pull_requests_snapshot("owner/repo")) == MAX_OPEN_PULL_REQUESTS
        saved_open_snapshot = open_pull_requests_snapshot
        selected_prs = tuple(
            {
                **implementation_pr,
                "number": 600 + index,
                "headRefOid": f"{index:040d}",
            }
            for index in range(33)
        )
        globals()["open_pull_requests_snapshot"] = lambda repo: selected_prs
        saved_selected_issue_payload = issue_payload
        globals()["issue_payload"] = lambda repo, issue: {"number": issue}
        exact_selected_snapshot, exact_selected = build_issueops_snapshot(
            "owner/repo", 10, {}, {}
        )
        assert len(exact_selected) == 33
        assert exact_selected_snapshot.selection_failures == ()
        globals()["issue_payload"] = saved_selected_issue_payload
        globals()["open_pull_requests_snapshot"] = saved_open_snapshot

        worker_saved = {
            name: globals()[name]
            for name in (
                "verify_live_candidate_head",
                "commit_status",
                "publish_implementation_status_for_pr",
            )
        }
        active_workers = 0
        maximum_active_workers = 0
        worker_lock = threading.Lock()

        def record_bounded_work() -> None:
            nonlocal active_workers, maximum_active_workers
            with worker_lock:
                active_workers += 1
                maximum_active_workers = max(maximum_active_workers, active_workers)
            time.sleep(0.0001)
            with worker_lock:
                active_workers -= 1

        pending_heads: list[int] = []
        pending_statuses: list[int] = []
        final_statuses: list[int] = []

        def fake_worker_head(repo: str, candidate: ImplementationStatusCandidate) -> None:
            record_bounded_work()
            pending_heads.append(candidate.number)

        def fake_worker_status(
            repo: str, sha: str, state: str, description: str
        ) -> None:
            record_bounded_work()
            assert state == "pending"
            pending_statuses.append(1)

        def fake_worker_final(
            repo: str,
            number: int,
            root: Path,
            issue_map: dict[str, tuple[Owner, ...]],
            release_graphs: dict[str, ReleaseGraph],
            pull_request: dict[str, object] | None = None,
            expected_pr_head_sha: str | None = None,
            snapshot: IssueOpsSnapshot | None = None,
            skip_pending: bool = False,
            run_id: int | None = None,
            generation_checked: bool = False,
        ) -> None:
            record_bounded_work()
            assert skip_pending
            final_statuses.append(number)

        globals()["verify_live_candidate_head"] = fake_worker_head
        globals()["commit_status"] = fake_worker_status
        globals()["publish_implementation_status_for_pr"] = fake_worker_final

        def worker_candidates(count: int) -> list[ImplementationStatusCandidate]:
            return [
                ImplementationStatusCandidate(
                    {"number": 800 + index, "headRefOid": f"{index:040d}"},
                    800 + index,
                    f"{index:040d}",
                )
                for index in range(count)
            ]

        thirty_three_candidates, selected_candidate_failures = (
            prepare_implementation_status_candidates(exact_selected)
        )
        assert selected_candidate_failures == []
        assert len(thirty_three_candidates) == 33
        assert pending_heads == []
        assert pending_statuses == []
        assert final_statuses == []

        generation_guard_calls: list[int] = []
        saved_generation_guard = newer_issueops_run_exists
        globals()["newer_issueops_run_exists"] = (
            lambda repo, run_id: generation_guard_calls.append(run_id) or False
        )
        final_statuses.clear()
        assert finalize_implementation_statuses(
            "owner/repo",
            Path("."),
            {},
            {},
            exact_selected_snapshot,
            thirty_three_candidates,
            run_id=100,
        ) == []
        assert generation_guard_calls == [100] * 5

        cross_batch_candidates = worker_candidates(9)
        cross_batch_generation = iter([False, True])
        generation_guard_calls.clear()
        globals()["newer_issueops_run_exists"] = (
            lambda repo, run_id: generation_guard_calls.append(run_id)
            or next(cross_batch_generation)
        )
        final_statuses.clear()
        cross_batch_failures = finalize_implementation_statuses(
            "owner/repo",
            Path("."),
            {},
            {},
            exact_selected_snapshot,
            cross_batch_candidates,
            run_id=100,
        )
        assert generation_guard_calls == [100, 100]
        assert len(final_statuses) == STATUS_WORKERS
        assert any("superseded before final status batch" in failure for failure in cross_batch_failures)

        def failing_generation_guard(repo: str, run_id: int) -> bool:
            raise SystemExit("injected generation API failure")

        globals()["newer_issueops_run_exists"] = failing_generation_guard
        generation_guard_calls.clear()
        final_statuses.clear()
        generation_failures = finalize_implementation_statuses(
            "owner/repo",
            Path("."),
            {},
            {},
            exact_selected_snapshot,
            worker_candidates(9),
            run_id=100,
        )
        assert generation_guard_calls == []
        assert final_statuses == []
        assert any("generation check failed before final status batch" in failure for failure in generation_failures)
        globals()["newer_issueops_run_exists"] = saved_generation_guard

        classification_prs = tuple(
            {
                **implementation_pr,
                "number": 1200 + index,
                "headRefOid": f"{index:040d}",
            }
            for index in range(MAX_OPEN_PULL_REQUESTS)
        )
        classified, classification_failures = implementation_prs_for_release_graphs(
            "owner/repo", classification_prs, dependent_graphs
        )
        assert classification_failures == []
        assert len(classified) == MAX_OPEN_PULL_REQUESTS
        assert final_statuses == []
        budget = issueops_request_budget(dependent_graphs)
        assert budget["status_writes"] == 49
        assert budget["generation_reads"] == 2
        assert budget["head_reads"] == 33
        assert budget["content_writes"] < MAX_CONTENT_WRITES_PER_MINUTE
        assert budget["request_total"] < MAX_GITHUB_REQUESTS_PER_HOUR
        assert budget["graph_reads"] <= MAX_GRAPH_REQUESTS_PER_EVENT

        admission_api_saved = gh_api_json
        admission_commit_saved = commit_status
        admission_gh_json_saved = gh_json
        admission_api_calls: list[list[str]] = []
        admission_status_calls: list[tuple[str, str]] = []

        def admission_api(args: list[str]) -> object:
            admission_api_calls.append(args)
            if args[0].endswith(f"/labels/{ISSUEOPS_ADMISSION_LABEL}"):
                return {"name": ISSUEOPS_ADMISSION_LABEL}
            return {}

        globals()["gh_api_json"] = admission_api
        globals()["commit_status"] = (
            lambda repo, sha, state, description: admission_status_calls.append((sha, state))
        )

        def admission_pr(number: int, admitted: bool) -> dict[str, object]:
            return {
                "number": number,
                "headRefOid": f"{number:040d}",
                "closingIssuesReferences": [
                    {
                        "number": 10,
                        "repository": {"name": "repo", "owner": {"login": "owner"}},
                    }
                ],
                "labels": (
                    [{"name": ISSUEOPS_ADMISSION_LABEL}] if admitted else []
                ),
            }

        seventeen = [admission_pr(1000 + index, index < MAX_ACTIVE_IMPLEMENTATION_PRS) for index in range(17)]
        first_admission = reconcile_admission_labels("owner/repo", seventeen)
        assert len(first_admission.admitted) == MAX_ACTIVE_IMPLEMENTATION_PRS
        assert [item["number"] for item in first_admission.overflow] == [1016]
        assert first_admission.evicted == ()
        admission_open_saved = open_pull_requests_snapshot
        globals()["open_pull_requests_snapshot"] = lambda repo: tuple(seventeen)
        admitted_ok, admitted_failure = implementation_pr_requires_admission(
            "owner/repo", seventeen[0], dependent_graphs
        )
        overflow_ok, overflow_failure = implementation_pr_requires_admission(
            "owner/repo", seventeen[-1], dependent_graphs
        )
        assert admitted_ok and admitted_failure is None
        assert not overflow_ok and "admission ceiling" in (overflow_failure or "")
        globals()["open_pull_requests_snapshot"] = admission_open_saved

        after_close = seventeen[1:]
        after_close[0] = admission_pr(1001, True)
        after_close[-1] = admission_pr(1016, False)
        freed_admission = reconcile_admission_labels("owner/repo", after_close)
        assert [item["number"] for item in freed_admission.admitted] == list(range(1001, 1017))
        assert freed_admission.overflow == ()
        assert any("labels" in " ".join(call) and "1016" not in " ".join(call) for call in admission_api_calls)

        reopened = [admission_pr(1000, False)] + [
            admission_pr(number, number <= 1016) for number in range(1001, 1017)
        ]
        reopened_admission = reconcile_admission_labels("owner/repo", reopened)
        assert [item["number"] for item in reopened_admission.overflow] == [1016]
        assert [item["number"] for item in reopened_admission.evicted] == [1016]
        globals()["gh_json"] = lambda args: {
            "number": 1016,
            "headRefOid": f"{1016:040d}",
        }
        assert publish_admission_failure_statuses("owner/repo", reopened_admission.evicted) == []
        assert admission_status_calls == [(f"{1016:040d}", "failure")]
        globals()["gh_api_json"] = admission_api_saved
        globals()["commit_status"] = admission_commit_saved
        globals()["gh_json"] = admission_gh_json_saved

        failure_candidates = worker_candidates(4)
        pending_attempts: list[int] = []

        def failing_worker_status(
            repo: str, sha: str, state: str, description: str
        ) -> None:
            pending_attempts.append(int(sha[-3:]))
            if pending_attempts[-1] == 1:
                raise SystemExit("injected pending worker failure")

        globals()["commit_status"] = failing_worker_status
        pending_failures = publish_pending_statuses("owner/repo", failure_candidates)
        assert len(pending_attempts) == 4
        assert any("injected pending worker failure" in failure for failure in pending_failures)

        final_failure_candidates = worker_candidates(4)
        final_attempts: list[int] = []

        def failing_worker_final(*args: object, **kwargs: object) -> None:
            number = int(args[1])
            final_attempts.append(number)
            if number == 801:
                raise SystemExit("injected final worker failure")

        globals()["commit_status"] = saved_commit_status
        globals()["publish_implementation_status_for_pr"] = failing_worker_final
        final_failures = finalize_implementation_statuses(
            "owner/repo",
            Path("."),
            {},
            {},
            exact_selected_snapshot,
            final_failure_candidates,
        )
        assert sorted(final_attempts) == sorted(
            candidate.number for candidate in final_failure_candidates
        )
        assert any("injected final worker failure" in failure for failure in final_failures)
        for name, value in worker_saved.items():
            globals()[name] = value

        race_candidates = [
            ImplementationStatusCandidate(
                {"number": 900, "headRefOid": "9" * 40}, 900, "9" * 40
            ),
            ImplementationStatusCandidate(
                {"number": 901, "headRefOid": "a" * 40}, 901, "a" * 40
            ),
        ]
        race_status_calls: list[tuple[int | str, str]] = []
        race_final_numbers: list[int] = []

        def race_verify(repo: str, candidate: ImplementationStatusCandidate) -> None:
            if candidate.number == 901:
                raise CandidateHeadChanged(candidate.number, candidate.expected_sha, "b" * 40)

        def race_commit(
            repo: str, sha: str, state: str, description: str
        ) -> None:
            race_status_calls.append((sha, state))

        def race_finalize(*args: object, **kwargs: object) -> None:
            race_final_numbers.append(int(args[1]))

        race_saved = {
            name: globals()[name]
            for name in (
                "verify_live_candidate_head",
                "commit_status",
                "publish_implementation_status_for_pr",
                "build_issueops_snapshot",
                "prepare_implementation_status_candidates",
            )
        }
        globals()["verify_live_candidate_head"] = race_verify
        globals()["commit_status"] = race_commit
        globals()["publish_implementation_status_for_pr"] = race_finalize
        globals()["build_issueops_snapshot"] = (
            lambda *args, **kwargs: (exact_selected_snapshot, [])
        )
        globals()["prepare_implementation_status_candidates"] = (
            lambda affected: (race_candidates, [])
        )
        try:
            publish_implementation_statuses_for_issue(
                "owner/repo", 10, Path("."), {}, {}
            )
        except SystemExit as error:
            assert "head changed before status publication" in str(error)
        else:
            raise AssertionError("raced candidate did not fail the aggregate refresh")
        assert [candidate.number for candidate in race_candidates] == [900]
        assert ("b" * 40, "failure") in race_status_calls
        assert ("9" * 40, "pending") in race_status_calls
        assert all(state != "pending" or sha != "a" * 40 for sha, state in race_status_calls)
        assert race_final_numbers == [900]
        for name, value in race_saved.items():
            globals()[name] = value

        coalesced_graphs = {
            "v0.5.0-00": ReleaseGraph(
                "v0.5.0-00", 492, {339: (), 492: (339,)}
            )
        }
        coalesced_pr_sha = "c" * 40
        coalesced_pr = {
            "number": 910,
            "headRefOid": coalesced_pr_sha,
            "closingIssuesReferences": [
                {
                    "number": 339,
                    "repository": {"name": "repo", "owner": {"login": "owner"}},
                }
            ],
            "author": {"login": "atlas"},
        }
        coalesced_status_states: list[str] = []
        coalesced_final_numbers: list[int] = []
        coalesced_saved = {
            name: globals()[name]
            for name in (
                "open_pull_requests_snapshot",
                "issue_payload",
                "release_graph_failures",
                "gh_json",
                "commit_status",
                "publish_implementation_status_for_pr",
            )
        }
        globals()["open_pull_requests_snapshot"] = lambda repo: (coalesced_pr,)
        globals()["issue_payload"] = (
            lambda repo, issue: {
                "number": issue,
                "state": "CLOSED",
                "milestone": {"title": "v0.5.0-00"},
            }
        )
        globals()["release_graph_failures"] = lambda *args, **kwargs: []

        def coalesced_gh_json(args: list[str]) -> object:
            if args[:2] == ["pr", "view"]:
                return {"number": 910, "headRefOid": coalesced_pr_sha}
            raise AssertionError(f"unexpected coalesced command: {args}")

        globals()["gh_json"] = coalesced_gh_json
        globals()["commit_status"] = (
            lambda repo, sha, state, description: coalesced_status_states.append(state)
        )

        def coalesced_final(
            repo: str,
            number: int,
            root: Path,
            issue_map: dict[str, tuple[Owner, ...]],
            release_graphs: dict[str, ReleaseGraph],
            *args: object,
            **kwargs: object,
        ) -> None:
            coalesced_final_numbers.append(number)

        globals()["publish_implementation_status_for_pr"] = coalesced_final
        # GitHub-native concurrency cancels stale A before it can finalize; this
        # local half proves surviving C still refreshes B's disjoint PR.
        coalesced_issue_events = {"A": 310, "B": 339, "C": 314}
        assert coalesced_issue_events["C"] == 314
        publish_implementation_statuses_for_issue(
            "owner/repo",
            coalesced_issue_events["C"],
            Path("."),
            {},
            coalesced_graphs,
            reconcile_all_release_graphs=True,
        )
        assert coalesced_status_states == ["pending"]
        assert coalesced_final_numbers == [910]
        for name, value in coalesced_saved.items():
            globals()[name] = value

        live_refresh_graphs = {
            "v0.5.0-00": ReleaseGraph(
                "v0.5.0-00", 492, {339: (), 492: (339,)}
            )
        }
        live_refresh_pr_sha = "d" * 40
        live_refresh_pr = {
            "number": 911,
            "headRefOid": live_refresh_pr_sha,
            "closingIssuesReferences": [
                {
                    "number": 339,
                    "repository": {"name": "repo", "owner": {"login": "owner"}},
                }
            ],
            "author": {"login": "atlas"},
        }
        live_refresh_states: list[str] = []
        live_refresh_calls = [0]
        live_refresh_b_changed = [False]
        live_refresh_saved = {
            name: globals()[name]
            for name in (
                "open_pull_requests_snapshot",
                "issue_payload",
                "release_graph_failures",
                "gh_json",
                "commit_status",
            )
        }
        globals()["open_pull_requests_snapshot"] = lambda repo: (live_refresh_pr,)

        def live_refresh_issue(repo: str, issue: int) -> dict[str, object]:
            return {
                "number": issue,
                "state": "OPEN" if live_refresh_b_changed[0] and issue == 339 else "CLOSED",
                "milestone": {"title": "v0.5.0-00"},
            }

        globals()["issue_payload"] = live_refresh_issue

        def live_refresh_graph_failures(*args: object, **kwargs: object) -> list[str]:
            live_refresh_calls[0] += 1
            if live_refresh_calls[0] == 1:
                # A captured its immutable snapshot; B's live graph change is now visible.
                live_refresh_b_changed[0] = True
                return []
            assert live_refresh_b_changed[0]
            return ["B changed a native release-graph edge after A's snapshot"]

        globals()["release_graph_failures"] = live_refresh_graph_failures
        globals()["gh_json"] = lambda args: (
            {"number": 911, "headRefOid": live_refresh_pr_sha}
            if args[:2] == ["pr", "view"]
            else (_ for _ in ()).throw(AssertionError(f"unexpected live-refresh command: {args}"))
        )
        globals()["commit_status"] = (
            lambda repo, sha, state, description: live_refresh_states.append(state)
        )
        try:
            publish_implementation_statuses_for_issue(
                "owner/repo",
                314,
                Path("."),
                {},
                live_refresh_graphs,
                reconcile_all_release_graphs=True,
            )
        except SystemExit as error:
            assert "affected implementation status publication failed" in str(error)
        else:
            raise AssertionError("live pre-final graph change did not fail the refresh")
        assert live_refresh_calls == [2]
        assert live_refresh_states == ["pending", "failure"]
        assert "success" not in live_refresh_states
        for name, value in live_refresh_saved.items():
            globals()[name] = value

        malformed_refresh = {"number": 495, "closingIssuesReferences": None}
        affected, refresh_failures = affected_implementation_prs(
            "owner/repo",
            10,
            (implementation_pr, malformed_refresh),
            dependent_graphs,
        )
        assert affected == [implementation_pr]
        assert any("PR #495" in failure for failure in refresh_failures)

        status_pr = {**implementation_pr, "closingIssuesReferences": []}
        status_calls.clear()
        globals()["gh_json"] = lambda args: status_pr
        publish_implementation_status_for_pr(
            "owner/repo", 494, Path("."), {}, {}
        )
        status_states = [
            argument.split("=", 1)[1]
            for call in status_calls
            for argument in call
            if argument.startswith("state=")
        ]
        assert status_states == ["pending", "success"]

        status_calls.clear()
        foreign_status_pr = {
            **implementation_pr,
            "closingIssuesReferences": [foreign_closing_reference],
        }

        def fake_foreign_status_gh_json(args: list[str]) -> object:
            assert args[:2] == ["pr", "view"]
            return foreign_status_pr

        globals()["gh_json"] = fake_foreign_status_gh_json
        try:
            publish_implementation_status_for_pr(
                "owner/repo", 494, Path("."), {}, {}
            )
        except SystemExit as error:
            assert "exact owner/repo repository identity" in str(error)
        else:
            raise AssertionError("foreign status reference was accepted")
        status_states = [
            argument.split("=", 1)[1]
            for call in status_calls
            for argument in call
            if argument.startswith("state=")
        ]
        assert status_states == ["failure"]

        globals()["issue_payload"] = lambda repo, issue: {
            "number": issue,
            "state": {10: "CLOSED", 11: "OPEN", 12: "CLOSED"}.get(issue, "CLOSED"),
        }
        rerun_commands: list[list[str]] = []
        globals()["run"] = lambda args: rerun_commands.append(args) or ""
        assert enforce_closed_issue_blockers("owner/repo", 10, dependent_graphs) == []
        assert rerun_commands == []
        try:
            enforce_closed_issue_blockers("owner/repo", 12, dependent_graphs)
        except SystemExit as error:
            assert "closed issue was reopened" in str(error)
            assert "blocker #11" in str(error)
        else:
            raise AssertionError("early-closed release root was accepted")
        assert rerun_commands == [["gh", "issue", "reopen", "12", "--repo", "owner/repo"]]

        relationship_graphs = {
            "v0.5.0-00": ReleaseGraph(
                "v0.5.0-00", 492, {339: (), 492: (339,)}
            )
        }
        relationship_pr_sha = "d" * 40
        relationship_pr = {
            "number": 500,
            "headRefOid": relationship_pr_sha,
            "closingIssuesReferences": [
                {
                    "number": 339,
                    "repository": {"name": "repo", "owner": {"login": "owner"}},
                }
            ],
            "author": {"login": "atlas"},
        }
        relationship_pr_two = {
            **relationship_pr,
            "number": 501,
            "headRefOid": "e" * 40,
        }
        behavior_saved = {
            name: globals()[name]
            for name in (
                "gh_json",
                "gh_api_json",
                "run",
                "issue_payload",
                "native_sub_issues",
                "native_issue_id",
                "check_openspec_tasks",
                "planned_issue_failures",
                "release_graph_failures",
                "build_issueops_snapshot",
                "prepare_implementation_status_candidates",
                "publish_pending_statuses",
                "finalize_implementation_statuses",
                "mutate_native_relationship",
                "reconcile_admission_labels",
            )
        }
        relationship_sub_issues = {339}
        status_api_calls: list[list[str]] = []
        lifecycle_events: list[str] = []

        def relationship_gh_json(args: list[str]) -> object:
            if args[:2] == ["pr", "list"]:
                return [relationship_pr, relationship_pr_two]
            if args[:2] == ["pr", "view"]:
                number = int(args[2])
                return {
                    "number": number,
                    "headRefOid": (
                        relationship_pr_sha if number == 500 else relationship_pr_two["headRefOid"]
                    ),
                }
            raise AssertionError(f"unexpected relationship snapshot command: {args}")

        def relationship_gh_api_json(args: list[str]) -> object:
            status_api_calls.append(args)
            lifecycle_events.append(
                "status:"
                + next(
                    argument.split("=", 1)[1]
                    for argument in args
                    if argument.startswith("state=")
                )
            )
            return {}

        def relationship_run(args: list[str]) -> str:
            assert args == [
                "gh",
                "api",
                "repos/owner/repo/issues/492/sub_issue",
                "--method",
                "DELETE",
                "-H",
                f"X-GitHub-Api-Version: {GITHUB_API_VERSION}",
                "-F",
                "sub_issue_id=1339",
            ]
            lifecycle_events.append("mutate")
            relationship_sub_issues.remove(339)
            return "{}"

        def relationship_issue_payload(repo: str, issue: int) -> dict[str, object]:
            return {
                "number": issue,
                "state": "CLOSED",
                "milestone": {"title": "v0.5.0-00"},
            }

        def relationship_graph_failures(
            repo: str,
            graphs: dict[str, ReleaseGraph],
            issue_map: dict[str, tuple[Owner, ...]],
            milestones: set[str] | None = None,
        ) -> list[str]:
            assert milestones == {"v0.5.0-00"}
            return (
                []
                if 339 in relationship_sub_issues
                else ["#492 is missing native sub-issue relation(s): #339"]
            )

        globals()["gh_json"] = relationship_gh_json
        globals()["gh_api_json"] = relationship_gh_api_json
        globals()["run"] = relationship_run
        globals()["issue_payload"] = relationship_issue_payload
        globals()["native_sub_issues"] = (
            lambda repo, issue: set(relationship_sub_issues)
        )
        globals()["native_issue_id"] = lambda repo, issue, label: issue + 1000
        globals()["check_openspec_tasks"] = lambda *args, **kwargs: []
        globals()["planned_issue_failures"] = lambda *args, **kwargs: []
        globals()["release_graph_failures"] = relationship_graph_failures
        globals()["reconcile_admission_labels"] = (
            lambda repo, implementation_prs: ImplementationAdmission(
                tuple(implementation_prs), (), (), (), True
            )
        )
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                492,
                339,
                "sub_issue",
                "remove",
                Path("."),
                {},
                relationship_graphs,
            )
        except SystemExit as error:
            assert "affected implementation status publication failed" in str(error)
        else:
            raise AssertionError("relationship drift did not fail child implementation status")
        assert relationship_sub_issues == set()
        assert lifecycle_events == [
            "status:pending",
            "status:pending",
            "mutate",
            "status:failure",
            "status:failure",
        ]
        assert len(status_api_calls) == 4
        status_states = [
            argument.split("=", 1)[1]
            for call in status_api_calls
            for argument in call
            if argument.startswith("state=")
        ]
        assert status_states == ["pending", "pending", "failure", "failure"]
        assert all(
            "context=issueops-implementation" in call
            for call in (" ".join(args) for args in status_api_calls)
        )
        status_paths = [args[0] for args in status_api_calls]
        assert sorted(status_paths) == sorted([
            f"repos/owner/repo/statuses/{relationship_pr_sha}",
            f"repos/owner/repo/statuses/{relationship_pr_two['headRefOid']}",
            f"repos/owner/repo/statuses/{relationship_pr_sha}",
            f"repos/owner/repo/statuses/{relationship_pr_two['headRefOid']}",
        ])
        assert "#492 is missing native sub-issue relation(s): #339" in " ".join(
            argument
            for call in status_api_calls
            for argument in call
            if argument.startswith("description=")
        )
        pending_failure_mutations: list[object] = []
        globals()["build_issueops_snapshot"] = lambda *args, **kwargs: (
            IssueOpsSnapshot((), {}, {}, ()), [relationship_pr]
        )
        globals()["prepare_implementation_status_candidates"] = lambda affected: (
            [ImplementationStatusCandidate(relationship_pr, 500, relationship_pr_sha)],
            [],
        )
        globals()["publish_pending_statuses"] = (
            lambda repo, candidates: (_ for _ in ()).throw(
                SystemExit("injected pending publication failure")
            )
        )
        globals()["mutate_native_relationship"] = (
            lambda *args: pending_failure_mutations.append(args)
        )
        try:
            mutate_native_relationship_and_revalidate(
                "owner/repo",
                492,
                339,
                "sub_issue",
                "remove",
                Path("."),
                {},
                relationship_graphs,
            )
        except SystemExit as error:
            assert "injected pending publication failure" in str(error)
        else:
            raise AssertionError("pending failure did not abort relationship mutation")
        assert pending_failure_mutations == []
        assert len(status_api_calls) == 4
        for name, value in behavior_saved.items():
            globals()[name] = value
    finally:
        globals()["milestone_issues"] = saved_milestone_issues
        globals()["native_blocked_by"] = saved_native_blocked_by
        globals()["native_sub_issues"] = saved_native_sub_issues
        globals()["native_parent_issue"] = saved_native_parent_issue
        globals()["native_issue_id"] = saved_native_issue_id
        globals()["issue_payload"] = saved_issue_payload
        globals()["gh_json"] = saved_gh_json
        globals()["run"] = saved_run
        globals()["gh_api_json"] = saved_gh_api_json
        subprocess.run = saved_subprocess_run
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
    issue_mode.add_argument(
        "--implementation-issue-reference",
        help="validate a JSON GitHub native closing issue reference and require its blockers to be closed",
    )
    parser.add_argument(
        "--publish-implementation-status-for-pr",
        type=int,
        help="publish the current-policy implementation status for one PR head",
    )
    parser.add_argument(
        "--expected-pr-head-sha",
        help="immutable PR head SHA from the triggering pull-request event",
    )
    parser.add_argument(
        "--publish-implementation-statuses-for-issue",
        type=int,
        help="publish current-policy implementation statuses for affected open PRs",
    )
    parser.add_argument(
        "--invalidate-implementation-statuses-for-issue",
        type=int,
        help="publish pending implementation statuses before queued finalization",
    )
    parser.add_argument(
        "--reconcile-all-release-graphs",
        action="store_true",
        help="reconcile every open implementation PR in every declared release graph",
    )
    parser.add_argument(
        "--enforce-implementation-admission",
        action="store_true",
        help="maintain and enforce the bounded native implementation admission set",
    )
    parser.add_argument(
        "--mutate-native-relationship",
        action="store_true",
        help="apply one authorized native relationship and revalidate affected PRs",
    )
    parser.add_argument("--native-relationship-kind")
    parser.add_argument("--native-relationship-operation")
    parser.add_argument("--native-relationship-issue")
    parser.add_argument("--native-related-issue")
    parser.add_argument(
        "--request-id",
        help="validated repository_dispatch correlation token for relationship outcomes",
    )
    parser.add_argument(
        "--enforce-closed-issue-blockers",
        type=int,
        help="reopen a declared issue that closed before its direct blockers",
    )
    parser.add_argument("--run-id", type=int, help="current IssueOps workflow run id")
    parser.add_argument("--skip-openspec", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if not args.repo:
        raise SystemExit("--repo is required unless --self-test is used")
    if args.run_id is not None and args.run_id <= 0:
        raise SystemExit("--run-id must be a positive IssueOps workflow run id")
    request_id = validate_request_id(args.request_id) if args.request_id is not None else None
    status_modes = [
        args.publish_implementation_status_for_pr,
        args.publish_implementation_statuses_for_issue,
        args.invalidate_implementation_statuses_for_issue,
    ]
    if args.mutate_native_relationship and (
        any(value is not None for value in status_modes)
        or args.enforce_closed_issue_blockers is not None
    ):
        raise SystemExit(
            "native relationship mutation cannot be combined with another status mode"
        )
    if sum(value is not None for value in status_modes) > 1:
        raise SystemExit("implementation status modes are mutually exclusive")
    if args.mutate_native_relationship:
        if request_id is None:
            raise SystemExit(
                "native relationship mutation requires a validated --request-id"
            )
        try:
            issue, related_issue = validate_native_relationship_request(
                args.native_relationship_kind,
                args.native_relationship_operation,
                args.native_relationship_issue,
                args.native_related_issue,
            )
            issue_map = load_issue_map(args.issue_map)
            release_graphs = load_release_graphs(args.issue_map, issue_map)
            mutate_native_relationship_and_revalidate(
                args.repo,
                issue,
                related_issue,
                args.native_relationship_kind,
                args.native_relationship_operation,
                Path(args.root),
                issue_map,
                release_graphs,
                run_id=args.run_id,
                request_id=request_id,
            )
        except BaseException:
            print(relationship_outcome(request_id, "failed"), file=sys.stderr)
            raise
        return
    if (
        any(value is not None for value in status_modes)
        or args.enforce_closed_issue_blockers is not None
    ):
        issue_map = load_issue_map(args.issue_map)
        release_graphs = load_release_graphs(args.issue_map, issue_map)
        root = Path(args.root)
        closure_error: SystemExit | None = None
        if args.enforce_closed_issue_blockers is not None:
            try:
                enforce_closed_issue_blockers(
                    args.repo, args.enforce_closed_issue_blockers, release_graphs
                )
            except SystemExit as error:
                # The issue was reopened before this error.  Publish status from
                # the repaired live state before rejecting the triggering event.
                closure_error = error
        try:
            if args.invalidate_implementation_statuses_for_issue is not None:
                invalidate_implementation_statuses_for_issue(
                    args.repo,
                    args.invalidate_implementation_statuses_for_issue,
                    issue_map,
                    release_graphs,
                    reconcile_all_release_graphs=args.reconcile_all_release_graphs,
                    enforce_admission=args.enforce_implementation_admission,
                )
            if args.publish_implementation_status_for_pr is not None:
                publish_implementation_status_for_pr(
                    args.repo,
                    args.publish_implementation_status_for_pr,
                    root,
                    issue_map,
                    release_graphs,
                    expected_pr_head_sha=args.expected_pr_head_sha,
                    run_id=args.run_id,
                )
            if args.publish_implementation_statuses_for_issue is not None:
                publish_implementation_statuses_for_issue(
                    args.repo,
                    args.publish_implementation_statuses_for_issue,
                    root,
                    issue_map,
                    release_graphs,
                    reconcile_all_release_graphs=args.reconcile_all_release_graphs,
                    run_id=args.run_id,
                    enforce_admission=args.enforce_implementation_admission,
                )
        except SystemExit as status_error:
            if closure_error is not None:
                raise SystemExit(f"{closure_error}\n{status_error}") from status_error
            raise
        if closure_error is not None:
            raise closure_error
        return

    root = Path(args.root)
    failures: list[str] = []
    issue_map = load_issue_map(args.issue_map)
    release_graphs = load_release_graphs(args.issue_map, issue_map)
    implementation_issue = args.implementation_issue
    if args.implementation_issue_reference is not None:
        try:
            implementation_reference = json.loads(args.implementation_issue_reference)
        except json.JSONDecodeError as error:
            raise SystemExit(
                f"GitHub closing issue reference was not valid JSON: {error}"
            ) from error
        implementation_issue = native_relation_issue_number(
            args.repo, implementation_reference, "closing issue reference"
        )
    issue_number = args.planned_issue or implementation_issue
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
                else implementation_issue,
            )
        )
    if target_issue is not None:
        failures.extend(
            planned_issue_failures(
                target_issue, issue_map, root
            )
        )
    if implementation_issue is not None and target_issue is not None:
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
