"""Purpose: Fail PR CI while GitHub Codex review threads remain unresolved."""

import argparse
import json
import subprocess
import sys


DEFAULT_CODEX_BOT_LOGINS = (
    "chatgpt-codex-connector",
    "chatgpt-codex-connector[bot]",
)

THREADS_QUERY = """
query($owner: String!, $name: String!, $number: Int!, $after: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $after) {
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          comments(first: 100) {
            nodes {
              author {
                login
              }
              url
            }
            pageInfo {
              hasNextPage
              endCursor
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"""

COMMENTS_QUERY = """
query($threadId: ID!, $after: String) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      comments(first: 100, after: $after) {
        nodes {
          author {
            login
          }
          url
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"""


def run(args):
    process = subprocess.run(
        args,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if process.returncode:
        raise SystemExit(
            f"command failed: {' '.join(args)}\n{process.stderr.strip()}"
        )
    return process.stdout


def gh_graphql(query, fields, raw_fields=None):
    command = ["gh", "api", "graphql", "-f", f"query={query}"]
    for key, value in fields.items():
        command.extend(["-f", f"{key}={value}"])
    for key, value in (raw_fields or {}).items():
        command.extend(["-F", f"{key}={value}"])
    return json.loads(run(command))


def split_repo(repo):
    parts = (repo or "").split("/", 1)
    if len(parts) != 2 or not all(parts):
        raise SystemExit("--repo must use OWNER/NAME format")
    return parts[0], parts[1]


def normalize_logins(logins):
    return {login.strip().lower() for login in logins if login.strip()}


def comment_author(comment):
    author = comment.get("author") or {}
    return (author.get("login") or "").lower()


def thread_comments(thread):
    comments = thread.get("comments") or {}
    return comments.get("nodes") or []


def thread_has_codex_comment(thread, codex_logins):
    return any(comment_author(comment) in codex_logins for comment in thread_comments(thread))


def unresolved_codex_threads(threads, codex_logins):
    return [
        thread
        for thread in threads
        if not thread.get("isResolved") and thread_has_codex_comment(thread, codex_logins)
    ]


def first_codex_comment(thread, codex_logins):
    for comment in thread_comments(thread):
        if comment_author(comment) in codex_logins:
            return comment
    return {}


def fetch_remaining_comments(thread):
    comments = thread_comments(thread)
    page_info = ((thread.get("comments") or {}).get("pageInfo") or {}).copy()
    while page_info.get("hasNextPage"):
        payload = gh_graphql(
            COMMENTS_QUERY,
            {
                "threadId": thread["id"],
                "after": page_info.get("endCursor") or "",
            },
        )
        node = payload.get("data", {}).get("node") or {}
        page = node.get("comments") or {}
        comments.extend(page.get("nodes") or [])
        page_info = page.get("pageInfo") or {}
    thread["comments"] = {
        "nodes": comments,
        "pageInfo": page_info,
    }
    return thread


def fetch_review_threads(repo, pr_number):
    owner, name = split_repo(repo)
    threads = []
    after = None
    while True:
        fields = {"owner": owner, "name": name}
        if after:
            fields["after"] = after
        payload = gh_graphql(THREADS_QUERY, fields, {"number": pr_number})
        pull_request = (
            payload.get("data", {})
            .get("repository", {})
            .get("pullRequest")
        )
        if pull_request is None:
            raise SystemExit(f"pull request not found: {repo}#{pr_number}")
        page = pull_request.get("reviewThreads") or {}
        page_threads = [fetch_remaining_comments(thread) for thread in page.get("nodes") or []]
        threads.extend(page_threads)
        page_info = page.get("pageInfo") or {}
        if not page_info.get("hasNextPage"):
            return threads
        after = page_info.get("endCursor")


def location(thread):
    path = thread.get("path") or "<unknown path>"
    line = thread.get("line")
    return f"{path}:{line}" if line is not None else path


def fail_unresolved(threads, codex_logins):
    unresolved = unresolved_codex_threads(threads, codex_logins)
    if not unresolved:
        print("codex-pr-review-gate: no unresolved GitHub Codex review threads")
        return

    print(
        "codex-pr-review-gate: unresolved GitHub Codex review thread(s) block merge:",
        file=sys.stderr,
    )
    for thread in unresolved:
        comment = first_codex_comment(thread, codex_logins)
        url = comment.get("url") or thread.get("id")
        outdated = str(bool(thread.get("isOutdated"))).lower()
        author = (comment.get("author") or {}).get("login") or "<unknown>"
        print(
            f"- {location(thread)} by {author}; outdated={outdated}; {url}",
            file=sys.stderr,
        )
    print(
        "Resolve the GitHub review thread after fixing or explicitly dispositioning it, then rerun the check.",
        file=sys.stderr,
    )
    raise SystemExit(1)


def self_test():
    codex_logins = normalize_logins(DEFAULT_CODEX_BOT_LOGINS)
    sample_threads = [
        {
            "isResolved": False,
            "isOutdated": False,
            "path": "src/main.rs",
            "line": 12,
            "comments": {
                "nodes": [
                    {
                        "author": {"login": "chatgpt-codex-connector"},
                        "url": "https://example.test/thread-1",
                    }
                ]
            },
        },
        {
            "isResolved": True,
            "path": "src/lib.rs",
            "comments": {
                "nodes": [
                    {
                        "author": {"login": "chatgpt-codex-connector[bot]"},
                        "url": "https://example.test/thread-2",
                    }
                ]
            },
        },
        {
            "isResolved": False,
            "path": "src/other.rs",
            "comments": {
                "nodes": [
                    {
                        "author": {"login": "human-reviewer"},
                        "url": "https://example.test/thread-3",
                    }
                ]
            },
        },
    ]
    unresolved = unresolved_codex_threads(sample_threads, codex_logins)
    assert len(unresolved) == 1
    assert location(unresolved[0]) == "src/main.rs:12"
    assert split_repo("styler-ai/ProjectAtlas") == ("styler-ai", "ProjectAtlas")
    assert "chatgpt-codex-connector[bot]" in codex_logins
    print("codex PR review gate self-test passed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="")
    parser.add_argument("--pr", type=int, default=0)
    parser.add_argument("--bot-login", action="append", default=[])
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return

    if not args.repo:
        raise SystemExit("--repo is required unless --self-test is used")
    if args.pr <= 0:
        raise SystemExit("--pr must be a positive pull request number")

    bot_logins = normalize_logins(args.bot_login or DEFAULT_CODEX_BOT_LOGINS)
    fail_unresolved(fetch_review_threads(args.repo, args.pr), bot_logins)


if __name__ == "__main__":
    main()
