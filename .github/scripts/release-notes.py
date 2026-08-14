"""Purpose: Generate ProjectAtlas release notes from merged PRs and linked issues."""

import json
import os
import re
import subprocess
import sys
from datetime import datetime
from html.parser import HTMLParser

from release_version import parse_release_version


def run(args, check=True):
    process = subprocess.run(args, capture_output=True, text=True)
    if check and process.returncode:
        raise SystemExit(
            f"command failed: {' '.join(args)}\n{process.stderr.strip()}"
        )
    return process


def clean(text):
    return " ".join((text or "").replace("\r", "").split())


SUMMARY_PLACEHOLDER = "Describe what this change does and why."


def note_title(text):
    title = re.sub(
        r"^(bug|feat|fix|docs|chore|test)(?:\([^)]+\))?!?:\s*",
        "",
        clean(text),
        flags=re.I,
    ).rstrip(".;")
    return title[:1].upper() + title[1:]


SECTIONS = ("New Features", "Bug Fixes", "Chores")


def previous_tag_from(tags, version):
    try:
        current = parse_release_version(version, source="release")
    except ValueError:
        return ""
    candidates = []
    for tag in tags:
        try:
            candidate = parse_release_version(tag, source="release")
        except ValueError:
            continue
        if not candidate.is_prerelease and candidate.numbers < current.numbers:
            candidates.append((candidate.numbers, tag))
    return max(candidates)[1] if candidates else ""


class SummaryHTMLParser(HTMLParser):
    BLOCKED = {"blockquote", "details", "table"}

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.blocked_depth = 0
        self.heading = []
        self.in_heading = False
        self.in_summary = False
        self.stopped = False
        self.list_depth = 0
        self.pre_depth = 0
        self.item = []
        self.in_root_item = False
        self.items = []

    def handle_starttag(self, tag, _attrs):
        tag = tag.lower()
        if tag in self.BLOCKED:
            self.blocked_depth += 1
            return
        if self.blocked_depth:
            return
        if tag == "h2" and not self.list_depth and not self.pre_depth:
            if self.in_summary:
                self.in_summary = False
                self.stopped = True
            self.in_heading = not self.stopped
            self.heading = []
            return
        if not self.in_summary or self.stopped:
            return
        if tag in {"ul", "ol"}:
            self.list_depth += 1
        elif tag == "pre":
            self.pre_depth += 1
        elif tag == "li" and self.list_depth == 1:
            self.in_root_item = True
            self.item = []
        elif tag == "br" and self.in_root_item and self.list_depth == 1:
            self.item.append(" ")

    def handle_endtag(self, tag):
        tag = tag.lower()
        if tag in self.BLOCKED:
            self.blocked_depth = max(0, self.blocked_depth - 1)
            return
        if self.blocked_depth:
            return
        if tag == "h2":
            if self.in_heading and clean("".join(self.heading)).lower() == "summary":
                self.in_summary = True
            self.in_heading = False
            return
        if not self.in_summary or self.stopped:
            return
        if tag == "li" and self.in_root_item and self.list_depth == 1:
            text = clean("".join(self.item))
            if text and text != SUMMARY_PLACEHOLDER:
                self.items.append(text)
            self.in_root_item = False
            self.item = []
        elif tag in {"ul", "ol"}:
            self.list_depth = max(0, self.list_depth - 1)
        elif tag == "pre":
            self.pre_depth = max(0, self.pre_depth - 1)

    def handle_data(self, data):
        if self.in_heading:
            self.heading.append(data)
        elif (
            self.in_summary
            and self.in_root_item
            and self.list_depth == 1
            and not self.blocked_depth
            and not self.pre_depth
        ):
            self.item.append(data)


def summary_from_html(body_html, fallback):
    parser = SummaryHTMLParser()
    parser.feed(body_html or "")
    parser.close()
    return parser.items[:3] or [fallback]


def section_for(title="", labels=(), fallback_title=""):
    names = {label.get("name", "") for label in labels}
    for candidate in (title, fallback_title):
        lowered = candidate.lower()
        if re.match(r"^(?:fix|bug)(?:\([^)]*\))?!?:", lowered):
            return "Bug Fixes"
        if re.match(r"^(?:feat|feature)(?:\([^)]*\))?!?:", lowered):
            return "New Features"
    if "type:bug" in names:
        return "Bug Fixes"
    if "type:feature" in names:
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


def pr_metadata(repo, number):
    owner, name = repo.split("/", 1)
    query = """
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      bodyHTML
      closingIssuesReferences(first: 100) {
        nodes {
          number
          title
          state
          url
          labels(first: 100) { nodes { name } }
        }
      }
    }
  }
}
"""
    payload = json.loads(
        run(
            [
                "gh",
                "api",
                "graphql",
                "-f",
                f"query={query}",
                "-F",
                f"owner={owner}",
                "-F",
                f"name={name}",
                "-F",
                f"number={number}",
            ]
        ).stdout
    )
    pull_request = payload["data"]["repository"]["pullRequest"]
    closed_issues = [
        {
            "number": item["number"],
            "title": item["title"],
            "html_url": item["url"],
            "labels": item["labels"]["nodes"],
        }
        for item in pull_request["closingIssuesReferences"]["nodes"]
        if item["state"] == "CLOSED"
    ]
    return pull_request["bodyHTML"], closed_issues


def write_notes(repo, version):
    start_tag = previous_tag(version)
    prs = merged_prs(repo, start_tag)
    sections = {name: [] for name in SECTIONS}
    changelog = []

    for pr in prs:
        author = pr.get("user", {}).get("login", "unknown")
        changelog.append(f"- #{pr['number']} {clean(pr['title'])} @{author}")
        body_html, closed_issues = pr_metadata(repo, pr["number"])
        if closed_issues:
            for item in closed_issues:
                section = section_for(item.get("title", ""), item.get("labels", []))
                sections[section].append(
                    f"- {note_title(item['title'])}. "
                    f"([#{item['number']}]({item['html_url']}), "
                    f"[#{pr['number']}]({pr['html_url']}))"
                )
        else:
            for line in summary_from_html(body_html, pr["title"]):
                section = section_for(line, pr.get("labels", []), pr["title"])
                sections[section].append(
                    f"- {note_title(line)}. "
                    f"([#{pr['number']}]({pr['html_url']}))"
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
    body_html = """
<blockquote>
  <h2>Summary</h2>
  <ul><li>fix: Do not publish a quoted summary.</li></ul>
</blockquote>
<h2>Summary</h2>
<pre><code>- fix: Do not publish example bullets.</code></pre>
<details><ul><li>fix: Do not publish collapsed bullets.</li></ul></details>
<ul>
  <li>
    feat: Persist repository navigation across sessions.
    <p>while avoiding repeated discovery.</p>
    <h2>across the whole repository</h2>
    <ul><li>chore: Do not publish nested implementation details.</li></ul>
  </li>
  <li>
    fix: Keep <code>release notes</code>, user-facing.
    <pre><code>cargo test --workspace</code></pre>
  </li>
</ul>
<h2>Verification</h2>
<ul><li>python release-notes.py --self-test</li></ul>
"""
    assert summary_from_html(body_html, "fallback") == [
        "feat: Persist repository navigation across sessions. while avoiding repeated discovery. across the whole repository",
        "fix: Keep release notes, user-facing.",
    ]
    assert summary_from_html("", "fallback") == ["fallback"]
    assert summary_from_html(
        f"<h2>Summary</h2><ul><li>{SUMMARY_PLACEHOLDER}</li></ul>",
        "fallback",
    ) == ["fallback"]
    assert note_title("bug: stale runtime remains") == "Stale runtime remains"
    assert note_title("docs(memory): specify the Memory Atlas") == "Specify the Memory Atlas"
    assert note_title("test(parser): keep cancellation bounded") == "Keep cancellation bounded"
    assert note_title("feat: publish complete generations;.") == "Publish complete generations"
    assert section_for("fix(db): reject stale paths") == "Bug Fixes"
    assert section_for("feat(cli): add root diagnostics") == "New Features"
    assert section_for("feat: add graph navigation", [{"name": "type:bug"}]) == "New Features"
    assert section_for("fix: reject stale paths", [{"name": "type:feature"}]) == "Bug Fixes"
    assert section_for("feat!: replace the public format") == "New Features"
    assert section_for("fix(parser)!: reject legacy input") == "Bug Fixes"
    assert section_for("fixtures exercise release-note formatting") == "Chores"
    assert section_for("bugbear keeps the parser honest") == "Chores"
    assert section_for("feature-gate remains disabled") == "Chores"
    assert section_for("Persistent navigation.", [], "feat(index): publish graph") == "New Features"
    assert section_for("fix: reject stale paths", [], "feat(index): publish graph") == "Bug Fixes"
    assert merged_after("2026-07-05T18:59:26Z", 1783277965)
    assert not merged_after("2026-07-05T18:59:26Z", 1783277966)
    tags = ["v4.8.8", "v4.8.9", "v4.9.0-rc1", "v4.9.0-rc2"]
    assert previous_tag_from(tags, "v4.9.0-rc1") == "v4.8.9"
    assert previous_tag_from(tags, "v4.9.0-rc3") == "v4.8.9"
    assert previous_tag_from(tags, "v4.9.0") == "v4.8.9"
    assert previous_tag_from(["v4.8.8", "v4.8.9"], "v4.9.0") == "v4.8.9"
    assert previous_tag_from(["v4.9.0"], "v4.9.0") == ""
    print("release notes self-test passed")


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    else:
        write_notes(os.environ["GITHUB_REPOSITORY"], os.environ["RELEASE_VERSION"])
