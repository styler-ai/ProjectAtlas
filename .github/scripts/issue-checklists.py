"""Verify GitHub issue checklists mirror OpenSpec tasks."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
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
    r"\b(?:must|requir\w*|enforc\w*|bind\w*|verif\w*|evidence|proof|gate)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\bexact[- ]head\b[^.\n!?]{0,120}"
    r"\b(?:must|requir\w*|enforc\w*|bind\w*|verif\w*|evidence|proof|gate)\b"
    r")"
)
EXACT_HEAD_NEGATION_RE = re.compile(
    r"(?i)(?:"
    r"\b(?:do not|don't|does not|doesn't|must not|should not|cannot|can't|no longer)"
    r"\s+(?:require|use|bind|demand|enforce)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\b(?:remove|removing|removed|reject|rejecting|rejected|avoid|avoiding|"
    r"drop|dropping|dropped|retire|retiring|retired|without)\b"
    r"[^.\n!?]{0,120}\bexact[- ]head\b"
    r"|"
    r"\bexact[- ]head\b[^.\n!?]{0,120}"
    r"\b(?:is|are)?\s*(?:not|no longer)\s+(?:required|used|needed|accepted)\b"
    r")"
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
            assertions = [clause]
            if len(EXACT_HEAD_PROOF_RE.findall(clause)) > 1:
                assertions = re.split(
                    r"\s+\b(?:and|or)\b\s+",
                    clause,
                    flags=re.IGNORECASE,
                )
            for assertion in assertions:
                if not EXACT_HEAD_PROOF_RE.search(assertion):
                    continue
                if EXACT_HEAD_NEGATION_RE.search(assertion):
                    continue
                if EXACT_HEAD_REQUIREMENT_RE.search(assertion):
                    return True
    return False


def github_heading_slug(heading: str) -> str:
    """Return the GitHub-style fragment for one plain Markdown heading."""

    heading = re.sub(r"\s+#+\s*$", "", clean(heading))
    return "".join(
        character
        for character in heading.casefold()
        if character.isalnum() or character in {" ", "-", "_"}
    ).replace(" ", "-")


def markdown_heading_fragments(text: str) -> set[str]:
    """Return rendered heading fragments, including GitHub duplicate suffixes."""

    fragments: set[str] = set()
    next_suffix: dict[str, int] = {}
    for heading in HEADING_RE.finditer(visible_markdown(text)):
        base = github_heading_slug(heading.group(2))
        if not base:
            continue
        suffix = next_suffix.get(base, 0)
        fragment = base if suffix == 0 else f"{base}-{suffix}"
        while fragment in fragments:
            suffix += 1
            fragment = f"{base}-{suffix}"
        fragments.add(fragment)
        next_suffix[base] = suffix + 1
    return fragments


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


def issue_payload(repo: str, number: int) -> dict[str, object]:
    payload = gh_json(
        [
            "issue",
            "view",
            str(number),
            "-R",
            repo,
            "--json",
            "body,title,state,url",
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


def architecture_diagram_link_failures(section: str, repo: str, root: Path) -> list[str]:
    """Validate durable local architecture-document links for one issue section."""

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
        if segments[2:5] != ["blob", "dev", "docs"]:
            failures.append(
                f"architecture diagram link {url!r} must use /blob/dev/docs/"
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
                f"architecture diagram link {url!r} must target one direct document under /blob/dev/docs/"
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
                fragments = markdown_heading_fragments(
                    candidate.read_text(encoding="utf-8")
                )
            except OSError as error:
                failures.append(
                    f"architecture diagram link {url!r} documentation could not be read: {error}"
                )
                continue
            fragment = unquote(parsed.fragment)
            if fragment not in fragments:
                failures.append(
                    f"architecture diagram link {url!r} has no matching GitHub-style heading fragment"
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
    repo: str, root: Path, issue_map: dict[str, tuple[Owner, ...]]
) -> list[str]:
    failures: list[str] = []
    for change, owners in sorted(issue_map.items()):
        path, tasks = local_tasks(root, change)
        for owner, expected in owner_slices(path, tasks, owners):
            issue = issue_payload(repo, owner.issue)
            remote = issue_checklist_tasks(issue)
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
            for failure in issue_contract_failures(issue, expected, repo, root):
                failures.append(f"#{owner.issue} issue contract {failure}")
            if issue.get("state") == "CLOSED" and any(
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

- [ ] 2.1 Same-level task subsection

## 2. Acceptance Criteria

- [ ] A numeric sibling without matching task IDs must stop the task section

## 2026 Status

- [ ] Another random checkbox should not count
"""
    expected = [
        (True, "1.1 Anchored task"),
        (False, "2.1 Same-level task subsection"),
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
- [System architecture](https://github.com/owner/repo/blob/dev/docs/projectatlas-3-architecture.md#architecture-views)
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
- [ ] 2.1 Same-level task subsection
"""
    self_test_root = Path(__file__).resolve().parents[2]

    def contract_failures(
        issue: dict[str, object], tasks: list[tuple[bool, str]]
    ) -> list[str]:
        return issue_contract_failures(issue, tasks, "owner/repo", self_test_root)

    assert contract_failures({"state": "OPEN", "body": issue_contract}, expected) == []
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
        "Proof must not require exact-head identity.",
        "Proof should not require exact-head identity.",
        "Proof cannot require exact-head identity.",
        "Proof does not require exact-head identity.",
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
        "- [ ] 2.1 Same-level task subsection",
        "- [x] 2.1 Same-level task subsection",
    )
    assert contract_failures(
        {"state": "OPEN", "body": completed_contract},
        [(True, "1.1 Anchored task"), (True, "2.1 Same-level task subsection")],
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
    missing_architecture_link = issue_contract.replace(
        "- [System architecture](https://github.com/owner/repo/blob/dev/docs/projectatlas-3-architecture.md#architecture-views)",
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
        "[Insecure architecture](http://github.com/owner/repo/blob/dev/docs/projectatlas-3-architecture.md)",
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
    sha_architecture = issue_contract.replace(
        "/blob/dev/", "/blob/0123456789abcdef0123456789abcdef01234567/"
    )
    assert any(
        "must use /blob/dev/docs/" in failure
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
        "#architecture-views", "#missing-architecture-view"
    )
    assert any(
        "no matching GitHub-style heading fragment" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": missing_architecture_fragment}, expected
        )
    )
    fragmentless_architecture = issue_contract.replace("#architecture-views", "")
    assert any(
        "must include a Markdown heading fragment" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": fragmentless_architecture}, expected
        )
    )
    duplicate_architecture = issue_contract.replace(
        "## Release Scope",
        "## Architecture Diagrams\n- [Second view](https://github.com/owner/repo/blob/dev/docs/agent-navigation.md#initial-task-discovery)\n## Release Scope",
    )
    assert any(
        "exactly one visible non-empty 'architecture diagrams' section" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": duplicate_architecture}, expected
        )
    )
    empty_architecture = issue_contract.replace(
        "## Architecture Diagrams\n- [System architecture](https://github.com/owner/repo/blob/dev/docs/projectatlas-3-architecture.md#architecture-views)\n",
        "## Architecture Diagrams\n",
    )
    assert any(
        "'architecture diagrams' section must not be empty" in failure
        for failure in contract_failures(
            {"state": "OPEN", "body": empty_architecture}, expected
        )
    )
    wrong_architecture_order = issue_contract.replace(
        "## Capabilities\nName the capability.\n## Architecture Diagrams\n- [System architecture](https://github.com/owner/repo/blob/dev/docs/projectatlas-3-architecture.md#architecture-views)\n",
        "## Architecture Diagrams\n- [System architecture](https://github.com/owner/repo/blob/dev/docs/projectatlas-3-architecture.md#architecture-views)\n## Capabilities\nName the capability.\n",
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
    print("issue checklist self-test passed")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="")
    parser.add_argument("--root", default=".")
    parser.add_argument("--issue-map", default="openspec/issue-map.json")
    parser.add_argument("--milestone", action="append", default=[])
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
    if not args.skip_openspec:
        failures.extend(check_openspec_tasks(args.repo, root, issue_map))
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
