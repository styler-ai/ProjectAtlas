"""Verify GitHub issue checklists mirror OpenSpec tasks."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


TASK_RE = re.compile(r"(?m)^[ ]{0,3}[-*]\s+\[([ xX])\]\s+(.+?)\s*$")
TASK_ID_RE = re.compile(r"^(\d+(?:\.\d+)*)\s+")
HEADING_RE = re.compile(r"(?m)^(#{1,6})\s+(.+?)\s*$")
TASK_SECTION_HEADING_RE = re.compile(r"^(\d+(?:\.\d+)*)\.\s+")
HTML_COMMENT_RE = re.compile(r"(?s)<!--.*?(?:-->|$)")
FENCE_RE = re.compile(r"^[ ]{0,3}(`{3,}|~{3,})")


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


def milestone_mapping_failures(
    milestone: str, issues: list[dict[str, object]], mapped_issues: set[int]
) -> list[str]:
    failures: list[str] = []
    for item in issues:
        number = positive_issue(item.get("number"), "issue number")
        if number not in mapped_issues:
            failures.append(
                f"#{number} in milestone {milestone} has no local OpenSpec mapping"
            )
    return failures


def check_milestone_complete(
    repo: str, milestone: str, mapped_issues: set[int]
) -> list[str]:
    failures: list[str] = []
    issues = milestone_issues(repo, milestone)
    if not issues:
        return [f"milestone {milestone!r} has no issues"]
    failures.extend(milestone_mapping_failures(milestone, issues, mapped_issues))
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
    assert milestone_mapping_failures(
        "v1.0.0-00", [{"number": 1}, {"number": 3}], {1, 2}
    ) == ["#3 in milestone v1.0.0-00 has no local OpenSpec mapping"]
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
