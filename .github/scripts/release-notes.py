"""Purpose: Generate ProjectAtlas release notes from merged PRs and linked issues."""

import json
import os
import re
import subprocess
import sys


def run(args, check=True):
    process = subprocess.run(args, capture_output=True, text=True)
    if check and process.returncode:
        raise SystemExit(
            f"command failed: {' '.join(args)}\n{process.stderr.strip()}"
        )
    return process


def clean(text):
    return " ".join((text or "").replace("\r", "").split())


def note_title(text):
    title = re.sub(r"^(bug|feat|fix|docs|chore):\s*", "", clean(text), flags=re.I)
    return title[:1].upper() + title[1:]


SECTIONS = ("New Features", "Bug Fixes", "Chores")


def pr_summary(body, fallback):
    lines = (body or "").splitlines()
    in_summary = False
    summary = []
    for raw in lines:
        line = raw.strip()
        if line.startswith("## "):
            if in_summary:
                break
            in_summary = line.lstrip("#").strip().lower() == "summary"
            continue
        if in_summary and line:
            if line.startswith(("- ", "* ")):
                line = line[2:].strip()
            summary.append(clean(line))
    if summary:
        return summary[:3]
    for raw in lines:
        line = raw.strip()
        if line and not line.startswith("#"):
            return [clean(line[2:] if line.startswith(("- ", "* ")) else line)]
    return [fallback]


def issue_numbers(body):
    seen = set()
    numbers = []
    for match in re.finditer(r"#([0-9]+)", body or ""):
        number = int(match.group(1))
        if number not in seen:
            seen.add(number)
            numbers.append(number)
    return numbers


def section_for(title="", labels=()):
    names = {label.get("name", "") for label in labels}
    lowered = title.lower()
    if "type:bug" in names or lowered.startswith(("fix", "bug")):
        return "Bug Fixes"
    if "type:feature" in names or lowered.startswith(("feat", "feature")):
        return "New Features"
    return "Chores"


def gh_json(endpoint):
    return json.loads(
        run(
            [
                "gh",
                "api",
                endpoint,
                "-H",
                "Accept: application/vnd.github+json",
            ]
        ).stdout
    )


def previous_tag(version):
    process = run(["git", "describe", "--tags", "--abbrev=0", f"{version}^"], False)
    return process.stdout.strip() if process.returncode == 0 else ""


def merged_prs(repo, start_tag):
    range_spec = f"{start_tag}..HEAD" if start_tag else "HEAD"
    shas = run(["git", "rev-list", "--reverse", range_spec]).stdout.splitlines()
    prs = []
    seen = set()
    for sha in shas:
        for pr in gh_json(f"/repos/{repo}/commits/{sha}/pulls"):
            number = pr.get("number")
            if pr.get("merged_at") and number not in seen:
                seen.add(number)
                prs.append(pr)
    return prs


def issue(repo, number):
    try:
        item = gh_json(f"/repos/{repo}/issues/{number}")
    except SystemExit:
        return None
    if "pull_request" in item:
        return None
    return item


def write_notes(repo, version):
    start_tag = previous_tag(version)
    prs = merged_prs(repo, start_tag)
    sections = {name: [] for name in SECTIONS}
    changelog = []

    for pr in prs:
        author = pr.get("user", {}).get("login", "unknown")
        changelog.append(f"- #{pr['number']} {clean(pr['title'])} @{author}")
        fixed = [item for item in (issue(repo, number) for number in issue_numbers(pr.get("body"))) if item]
        if fixed:
            for item in fixed:
                section = section_for(item.get("title", ""), item.get("labels", []))
                sections[section].append(
                    f"- {note_title(item['title'])}. ([#{item['number']}]({item['html_url']}), [#{pr['number']}]({pr['html_url']}))"
                )
        else:
            section = section_for(pr.get("title", ""))
            for line in pr_summary(pr.get("body"), pr["title"]):
                sections[section].append(f"- {line} ([#{pr['number']}]({pr['html_url']}))")

    wrote_section = False
    for name in SECTIONS:
        items = sections[name]
        if not items:
            continue
        wrote_section = True
        print(f"## {name}")
        print()
        print("\n".join(items))
        print()
    if not wrote_section:
        print("## Chores")
        print()
        print("- No user-facing changes were identified for this release.")
        print()

    print("## Changelog")
    print()
    if start_tag:
        print(f"Full Changelog: https://github.com/{repo}/compare/{start_tag}...{version}")
    else:
        print(f"Full Changelog: https://github.com/{repo}/commits/{version}")
    print()
    print("\n".join(changelog))


def self_test():
    body = """## Summary

- First fix.
- Second fix.

## Verification

- cargo test
"""
    assert pr_summary(body, "fallback") == ["First fix.", "Second fix."]
    assert issue_numbers("Fixes #177, #180 and resolves #188.") == [177, 180, 188]
    assert note_title("bug: stale runtime remains") == "Stale runtime remains"
    assert section_for("fix(db): reject stale paths") == "Bug Fixes"
    assert section_for("feat(cli): add root diagnostics") == "New Features"
    print("release notes self-test passed")


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    else:
        write_notes(os.environ["GITHUB_REPOSITORY"], os.environ["RELEASE_VERSION"])
