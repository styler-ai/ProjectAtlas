"""Purpose: Generate ProjectAtlas release notes from merged PRs and linked issues."""

import json
import os
import re
import subprocess
import sys
from datetime import datetime


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
    title = re.sub(
        r"^(bug|feat|fix|docs|chore|test)(?:\([^)]+\))?!?:\s*",
        "",
        clean(text),
        flags=re.I,
    )
    return title[:1].upper() + title[1:]


SECTIONS = ("New Features", "Bug Fixes", "Chores")


def semver_key(tag):
    match = re.fullmatch(r"v([0-9]+)\.([0-9]+)\.([0-9]+)", tag or "")
    return tuple(int(part) for part in match.groups()) if match else None


def previous_tag_from(tags, version):
    current = semver_key(version)
    if current is None:
        return ""
    candidates = []
    for tag in tags:
        key = semver_key(tag)
        if key and key < current:
            candidates.append((key, tag))
    return max(candidates)[1] if candidates else ""


def issue_numbers(body):
    seen = set()
    numbers = []
    for line in (body or "").splitlines():
        if not re.search(r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\b", line, re.I):
            continue
        for match in re.finditer(r"#([0-9]+)", line):
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
    process = run(["git", "tag", "--merged", "HEAD"], False)
    return previous_tag_from(process.stdout.splitlines(), version) if process.returncode == 0 else ""


def merged_after(timestamp, cutoff):
    return bool(timestamp) and datetime.fromisoformat(
        timestamp.replace("Z", "+00:00")
    ).timestamp() > cutoff


def merged_prs(repo, start_tag):
    range_spec = f"{start_tag}..HEAD" if start_tag else "HEAD"
    cutoff = (
        int(run(["git", "show", "-s", "--format=%ct", start_tag]).stdout.strip())
        if start_tag
        else 0
    )
    shas = run(["git", "rev-list", "--reverse", range_spec]).stdout.splitlines()
    prs = []
    seen = set()
    for sha in shas:
        for pr in gh_json(f"/repos/{repo}/commits/{sha}/pulls"):
            number = pr.get("number")
            if number not in seen and merged_after(pr.get("merged_at"), cutoff):
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
        closed_issues = [
            item
            for item in (
                issue(repo, number) for number in issue_numbers(pr.get("body"))
            )
            if item
        ]
        links = [
            f"[#{item['number']}]({item['html_url']})" for item in closed_issues
        ]
        links.append(f"[#{pr['number']}]({pr['html_url']})")
        section = section_for(pr.get("title", ""), pr.get("labels", []))
        sections[section].append(
            f"- {note_title(pr['title']).rstrip('.')}. ({', '.join(links)})"
        )

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
    assert issue_numbers("Related #10.\nFixes #177, #180 and resolves #188.\nSee #190.") == [
        177,
        180,
        188,
    ]
    assert note_title("bug: stale runtime remains") == "Stale runtime remains"
    assert note_title("docs(memory): specify the Memory Atlas") == "Specify the Memory Atlas"
    assert note_title("test(parser): keep cancellation bounded") == "Keep cancellation bounded"
    assert section_for("fix(db): reject stale paths") == "Bug Fixes"
    assert section_for("feat(cli): add root diagnostics") == "New Features"
    assert merged_after("2026-07-05T18:59:26Z", 1783277965)
    assert not merged_after("2026-07-05T18:59:26Z", 1783277966)
    assert previous_tag_from(["v0.3.15", "v0.3.16"], "v0.3.17") == "v0.3.16"
    assert previous_tag_from(["v0.3.15", "v0.3.16", "v0.3.17"], "v0.3.17") == "v0.3.16"
    print("release notes self-test passed")


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    else:
        write_notes(os.environ["GITHUB_REPOSITORY"], os.environ["RELEASE_VERSION"])
