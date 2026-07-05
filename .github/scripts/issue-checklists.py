"""Purpose: Verify GitHub issue checklists mirror OpenSpec tasks before release."""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


TASK_RE = re.compile(r"(?m)^\s*[-*]\s+\[([ xX])\]\s+(.+?)\s*$")
HEADING_RE = re.compile(r"(?m)^(#{1,6})\s+(.+?)\s*$")
TASK_SECTION_HEADING_RE = re.compile(r"^\d+(?:\.\d+)*\.?\s+")


def run(args):
    process = subprocess.run(args, capture_output=True, text=True)
    if process.returncode:
        raise SystemExit(
            f"command failed: {' '.join(args)}\n{process.stderr.strip()}"
        )
    return process.stdout


def gh_json(args):
    return json.loads(run(["gh", *args]))


def gh_api_json(args):
    return json.loads(run(["gh", "api", *args]))


def clean(text):
    return " ".join((text or "").replace("\r", "").split())


def parse_tasks(text):
    tasks = []
    for match in TASK_RE.finditer(text or ""):
        checked = match.group(1).lower() == "x"
        tasks.append((checked, clean(match.group(2))))
    return tasks


def heading_matches_openspec_tasks(heading):
    normalized = clean(heading).lower()
    return normalized in {"openspec tasks", "openspec task checklist"}


def heading_is_task_subsection(heading):
    return TASK_SECTION_HEADING_RE.match(clean(heading)) is not None


def parse_section_tasks(text, heading_predicate):
    tasks = []
    headings = list(HEADING_RE.finditer(text or ""))
    for index, heading in enumerate(headings):
        if not heading_predicate(heading.group(2)):
            continue
        level = len(heading.group(1))
        end = len(text or "")
        for next_heading in headings[index + 1 :]:
            if len(next_heading.group(1)) <= level and not heading_is_task_subsection(
                next_heading.group(2)
            ):
                end = next_heading.start()
                break
        tasks.extend(parse_tasks((text or "")[heading.end() : end]))
    return tasks


def load_issue_map(path):
    path = Path(path)
    with open(path, encoding="utf-8") as handle:
        payload = json.load(handle)
    changes = payload.get("changes", {})
    if not isinstance(changes, dict):
        raise SystemExit(f"{path} must contain a changes object")
    mapped = {str(change): int(issue) for change, issue in changes.items()}
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
    return mapped


def issue_payload(repo, number):
    return gh_json(
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


def issue_checklist_tasks(issue, heading_predicate):
    tasks = []
    tasks.extend(parse_section_tasks(issue.get("body", ""), heading_predicate))
    return tasks


def local_tasks(root, change):
    path = root / "openspec" / "changes" / change / "tasks.md"
    if not path.exists():
        raise SystemExit(f"OpenSpec tasks file missing for {change}: {path}")
    tasks = parse_tasks(path.read_text(encoding="utf-8"))
    if not tasks:
        raise SystemExit(f"OpenSpec tasks file has no checkbox tasks: {path}")
    return path, tasks


def check_openspec_tasks(repo, root, issue_map):
    failures = []
    for change, issue_number in sorted(issue_map.items()):
        path, expected_tasks = local_tasks(root, change)
        issue = issue_payload(repo, issue_number)
        remote_tasks = issue_checklist_tasks(issue, heading_matches_openspec_tasks)
        remote_states = {}
        for checked, task in remote_tasks:
            remote_states.setdefault(task, set()).add(checked)
        for checked, task in expected_tasks:
            if checked not in remote_states.get(task, set()):
                state = "x" if checked else " "
                failures.append(
                    f"#{issue_number} does not show `- [{state}] {task}` from {path}"
                )
        print(
            f"#{issue_number} {change}: local {len(expected_tasks)} / "
            f"remote {len(remote_tasks)} / checked {sum(1 for checked, _ in remote_tasks if checked)}"
        )
        if issue.get("state") == "CLOSED" and any(not checked for checked, _ in remote_tasks):
            failures.append(f"#{issue_number} is closed but still has unchecked tasks")
    return failures


def repo_parts(repo):
    parts = repo.split("/", 1)
    if len(parts) != 2 or not parts[0] or not parts[1]:
        raise SystemExit(f"--repo must be OWNER/REPO, got {repo!r}")
    return parts


def flatten_paginated_response(payload):
    if not isinstance(payload, list):
        raise SystemExit("expected GitHub API pagination response to be a JSON list")
    if all(isinstance(page, list) for page in payload):
        return [item for page in payload for item in page]
    return payload


def milestone_number(repo, milestone):
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
    if not matches:
        return None
    return matches[0].get("number")


def milestone_issues(repo, milestone):
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
    return [
        {
            "number": item["number"],
            "title": item.get("title", ""),
            "state": item.get("state", ""),
            "url": item.get("html_url", ""),
        }
        for item in flatten_paginated_response(payload)
        if "pull_request" not in item
    ]


def check_milestone_complete(repo, milestone):
    failures = []
    issues = milestone_issues(repo, milestone)
    if not issues:
        failures.append(f"milestone {milestone!r} has no issues")
        return failures
    for item in issues:
        issue = issue_payload(repo, item["number"])
        tasks = issue_checklist_tasks(issue, heading_matches_openspec_tasks)
        checked = sum(1 for is_checked, _ in tasks if is_checked)
        unchecked = len(tasks) - checked
        print(
            f"#{item['number']} {issue.get('state')}: "
            f"tasks {len(tasks)} / checked {checked} / open {unchecked}"
        )
        if not tasks:
            failures.append(
                f"#{item['number']} in milestone {milestone} has no visible checklist tasks"
            )
        if unchecked:
            failures.append(
                f"#{item['number']} in milestone {milestone} has {unchecked} unchecked tasks"
            )
    return failures


def self_test():
    sample = """
- [x] 1.1 Done task
  - [ ] Nested item
* [ ] 2.1 Open   task
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

## Acceptance Criteria

- [ ] Another random checkbox should not count
"""
    comment = """
## OpenSpec Task Checklist Update

- [x] 2.1 Comment task
"""
    completed = """
## Completed v0.3.24 Task Checklist

- [x] Backfilled shipped task
"""
    assert parse_section_tasks(issue_body, heading_matches_openspec_tasks) == [
        (True, "1.1 Anchored task"),
        (False, "2.1 Same-level task subsection"),
    ]
    issue_with_comment = {"body": issue_body, "comments": [{"body": comment}]}
    assert issue_checklist_tasks(issue_with_comment, heading_matches_openspec_tasks) == [
        (True, "1.1 Anchored task"),
        (False, "2.1 Same-level task subsection"),
    ]
    assert parse_section_tasks(completed, heading_matches_openspec_tasks) == []
    assert repo_parts("owner/repo") == ["owner", "repo"]
    assert flatten_paginated_response([[{"number": 1}], [{"number": 2}]]) == [
        {"number": 1},
        {"number": 2},
    ]
    assert flatten_paginated_response([{"number": 3}]) == [{"number": 3}]
    assert clean("a\r\n  b") == "a b"
    print("issue checklist self-test passed")


def main():
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
    failures = []
    if not args.skip_openspec:
        failures.extend(check_openspec_tasks(args.repo, root, load_issue_map(args.issue_map)))
    for milestone in args.milestone:
        failures.extend(check_milestone_complete(args.repo, milestone))

    if failures:
        print("\nIssue checklist validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
