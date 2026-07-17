"""Validate and render ProjectAtlas OpenSpec issue authority.

GitHub access stays in this adapter. Issue bodies, pull-request text, artifacts,
and verification-plan commands are always treated as data and are never
executed.
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import html
import json
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path, PurePosixPath
from unittest.mock import patch
from urllib.parse import quote


SCHEMA_VERSION = 2
PLAN_SCHEMA_VERSION = 1
EVIDENCE_SCHEMA_VERSION = 1
PROVENANCE_SCHEMA_VERSION = 1
RENDER_SCHEMA_VERSION = 1
BODY_CHARACTER_LIMIT = 65_536
MAX_ARTIFACT_BYTES = 10_485_760
TASK_HEADING_NAMES = {"openspec tasks", "openspec task checklist"}
INLINE_CODE_RE = re.compile(r"`[^`\r\n]*`")
TRUSTED_COMMENT_ASSOCIATIONS = {"OWNER", "MEMBER", "COLLABORATOR"}
TASK_LINE_RE = re.compile(
    r"^- \[([ xX])\] (\d+(?:\.\d+)*) (\S(?:.*\S)?)$"
)
HEADING_RE = re.compile(r"^(#{1,6}) (\S(?:.*\S)?)$")
NUMBERED_HEADING_RE = re.compile(r"^\d+(?:\.\d+)*\.?\s+")
TASK_ID_RE = re.compile(r"^\d+(?:\.\d+)*$")
BACKTICK_TEST_ID_RE = re.compile(
    r"`([A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*-UT-\d+(?:\.\d+)*)`"
)
TAG_TEST_ID_RE = re.compile(r"\[UT:([A-Z][A-Z0-9-]*-\d+(?:\.\d+)*)\]")
PR_ISSUE_RE = re.compile(r"(?m)^OpenSpec-Issue: #([1-9]\d*)\s*$")
PR_TASK_RE = re.compile(
    r"(?m)^OpenSpec-Task: ([a-z0-9][a-z0-9-]*)/"
    r"(\d+(?:\.\d+)*)(?:\.\.(\d+(?:\.\d+)*))?\s*$"
)
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
REPOSITORY_RE = re.compile(
    r"^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,99})/"
    r"[A-Za-z0-9](?:[A-Za-z0-9._-]{0,99})$"
)
MARKER_RE = re.compile(
    r"^<!-- projectatlas-task-evidence:v1 change=([a-z0-9][a-z0-9-]*) "
    r"section=(\d+(?:\.\d+)*) -->$"
)
MARKER_PREFIX = "<!-- projectatlas-task-evidence:v1 "
FINAL_VERIFICATION_CHANGES = {
    "advance-rust-repository-intelligence",
    "enforce-rust-test-quality-gates",
}
SOURCE_DEFINITION_PATTERNS = {
    ".rs": r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+{anchor}\b",
    ".py": r"^\s*(?:async\s+)?def\s+{anchor}\b",
    ".js": r"^\s*(?:export\s+)?(?:(?:async\s+)?function|const|let|var)\s+{anchor}\b",
    ".jsx": r"^\s*(?:export\s+)?(?:(?:async\s+)?function|const|let|var)\s+{anchor}\b",
    ".ts": r"^\s*(?:export\s+)?(?:(?:async\s+)?function|const|let|var)\s+{anchor}\b",
    ".tsx": r"^\s*(?:export\s+)?(?:(?:async\s+)?function|const|let|var)\s+{anchor}\b",
    ".ps1": r"^\s*function\s+{anchor}\b",
    ".sh": r"^\s*(?:function\s+)?{anchor}\s*\(\s*\)",
}
TQG_CHANGE = "enforce-rust-test-quality-gates"
TQG_TASK_PATH = "openspec/changes/enforce-rust-test-quality-gates/tasks.md"
TQG_REQUIRED_SECTION_INPUTS = {
    "1": {
        "test-quality.toml",
        ".config/nextest.toml",
        ".cargo/mutants.toml",
        "openspec/changes/enforce-rust-test-quality-gates/proposal.md",
        "openspec/changes/enforce-rust-test-quality-gates/design.md",
        "openspec/changes/enforce-rust-test-quality-gates/specs/rust-test-quality-gates/spec.md",
        TQG_TASK_PATH,
    },
    "2": {
        ".github/scripts/issue-checklists.py",
        ".github/fixtures/issueops/cases.json",
        ".github/pull_request_template.md",
        ".github/workflows/06-task-evidence-render.yml",
        "openspec/issue-map.json",
        "openspec/task-verification.json",
        "openspec/task-evidence.json",
        TQG_TASK_PATH,
    },
    "3": {
        "Cargo.lock",
        "crates/projectatlas-lints/Cargo.toml",
        "crates/projectatlas-lints/src/main.rs",
        "crates/projectatlas-lints/src/test_quality.rs",
        TQG_TASK_PATH,
    },
    "4": {
        ".config/nextest.toml",
        ".github/workflows/ci.yml",
        "crates/projectatlas-lints/src/test_quality.rs",
        "crates/projectatlas-cli/tests/e2e.rs",
        TQG_TASK_PATH,
    },
    "5": {
        "test-quality.toml",
        ".github/workflows/ci.yml",
        "crates/projectatlas-lints/src/test_quality.rs",
        "crates/projectatlas-cli/tests/e2e.rs",
        TQG_TASK_PATH,
    },
    "6": {
        ".cargo/mutants.toml",
        "test-quality.toml",
        ".github/workflows/ci.yml",
        "crates/projectatlas-lints/src/test_quality.rs",
        "crates/projectatlas-cli/tests/e2e.rs",
        TQG_TASK_PATH,
    },
    "7": {
        ".cargo/mutants.toml",
        "test-quality.toml",
        ".github/workflows/05-full-mutation.yml",
        "crates/projectatlas-lints/src/test_quality.rs",
        "crates/projectatlas-cli/tests/e2e.rs",
        TQG_TASK_PATH,
    },
    "8": {
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/workflows/03-auto-release.yml",
        ".github/workflows/05-full-mutation.yml",
        ".github/workflows/06-task-evidence-render.yml",
        ".github/workflows/07-quality-failure-smoke.yml",
        ".github/scripts/issue-checklists.py",
        "crates/projectatlas-cli/tests/e2e.rs",
        TQG_TASK_PATH,
    },
    "9": {
        ".githooks/pre-push",
        "docs/workflow.md",
        "test-quality.toml",
        ".config/nextest.toml",
        ".cargo/mutants.toml",
        ".github/workflows/ci.yml",
        ".github/workflows/05-full-mutation.yml",
        ".github/workflows/06-task-evidence-render.yml",
        ".github/workflows/07-quality-failure-smoke.yml",
        "crates/projectatlas-cli/tests/e2e.rs",
        TQG_TASK_PATH,
    },
}
TQG_REQUIRED_SECTION_INPUTS["10"] = set().union(
    *TQG_REQUIRED_SECTION_INPUTS.values(),
    {
        "Cargo.toml",
        "crates/projectatlas-lints/Cargo.toml",
        "crates/projectatlas-lints/src/main.rs",
        "openspec/issue-map.json",
        "openspec/task-verification.json",
        "openspec/task-evidence.json",
    },
)


class ValidationError(ValueError):
    """Raised when repository or GitHub IssueOps data violates the contract."""


@dataclass(frozen=True)
class Task:
    """One canonical OpenSpec checkbox row."""

    checked: bool
    task_id: str
    text: str
    test_ids: tuple[str, ...]
    section: str


@dataclass(frozen=True)
class Owner:
    """One inclusive authoritative GitHub task range."""

    issue: int
    first_task: str
    last_task: str


@dataclass(frozen=True)
class ChangeMapping:
    """Versioned authority mapping for one OpenSpec change."""

    contract: str
    primary_issue: int
    owners: tuple[Owner, ...]


@dataclass(frozen=True)
class ScopeRow:
    """One explicitly declared pull-request task range."""

    change: str
    first_task: str
    last_task: str


def run(args: list[str], *, input_text: str | None = None) -> str:
    """Run one fixed argument vector without a shell."""

    process = subprocess.run(
        args,
        input=input_text,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    if process.returncode:
        raise ValidationError(
            f"command failed: {json.dumps(args)}\n{process.stderr.strip()}"
        )
    return process.stdout


def gh_json(args: list[str]) -> object:
    """Run a fixed `gh` argument vector and decode its JSON output."""

    return json.loads(run(["gh", *args]))


def gh_api_json(args: list[str]) -> object:
    """Run a fixed `gh api` argument vector and decode its JSON output."""

    return json.loads(run(["gh", "api", *args]))


def gh_api_write(endpoint: str, method: str, payload: dict[str, object]) -> object:
    """Send one JSON mutation to a previously validated GitHub endpoint."""

    return json.loads(
        run(
            ["gh", "api", endpoint, "--method", method, "--input", "-"],
            input_text=json.dumps(payload, ensure_ascii=True),
        )
    )


def require_object(value: object, name: str) -> dict[str, object]:
    """Return a JSON object or fail with its owning field name."""

    if not isinstance(value, dict):
        raise ValidationError(f"{name} must be an object")
    return value


def require_array(value: object, name: str) -> list[object]:
    """Return a JSON array or fail with its owning field name."""

    if not isinstance(value, list):
        raise ValidationError(f"{name} must be an array")
    return value


def require_exact_keys(
    value: dict[str, object], required: set[str], name: str, optional: set[str] | None = None
) -> None:
    """Reject missing and unknown schema fields."""

    optional = optional or set()
    missing = required - value.keys()
    unknown = value.keys() - required - optional
    if missing:
        raise ValidationError(f"{name} is missing fields: {', '.join(sorted(missing))}")
    if unknown:
        raise ValidationError(f"{name} has unknown fields: {', '.join(sorted(unknown))}")


def require_string(value: object, name: str) -> str:
    """Return a non-empty string without NUL characters."""

    if not isinstance(value, str) or not value or "\0" in value:
        raise ValidationError(f"{name} must be a non-empty string without NUL bytes")
    return value


def require_positive_int(value: object, name: str) -> int:
    """Return a positive integer while rejecting booleans."""

    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise ValidationError(f"{name} must be a positive integer")
    return value


def normalize_newlines(text: str) -> str:
    """Normalize transport line endings without hiding whitespace drift."""

    return text.replace("\r\n", "\n").replace("\r", "\n")


def c_like_code_lines(source: str) -> list[str]:
    """Mask strings and comments while preserving source line numbers."""

    output: list[str] = []
    current: list[str] = []
    index = 0
    block_comment_depth = 0
    quote_character: str | None = None
    raw_terminator: str | None = None
    escaped = False
    while index < len(source):
        character = source[index]
        following = source[index : index + 2]
        if character == "\n":
            output.append("".join(current))
            current = []
            if quote_character not in {'"', "`"} and raw_terminator is None:
                quote_character = None
            index += 1
            continue
        if raw_terminator is not None:
            if source.startswith(raw_terminator, index):
                current.extend(" " * len(raw_terminator))
                index += len(raw_terminator)
                raw_terminator = None
            else:
                current.append(" ")
                index += 1
            continue
        if block_comment_depth:
            if following == "/*":
                current.extend("  ")
                index += 2
                block_comment_depth += 1
                continue
            if following == "*/":
                current.extend("  ")
                index += 2
                block_comment_depth -= 1
            else:
                current.append(" ")
                index += 1
            continue
        if quote_character is not None:
            current.append(" ")
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == quote_character:
                quote_character = None
            index += 1
            continue
        raw_match = re.match(r'(?:br|r)(?P<hashes>#{0,255})"', source[index:])
        if raw_match:
            token = raw_match.group(0)
            current.extend(" " * len(token))
            raw_terminator = '"' + raw_match.group("hashes")
            index += len(token)
            continue
        if following == "/*":
            current.extend("  ")
            index += 2
            block_comment_depth = 1
            continue
        if following == "//":
            while index < len(source) and source[index] != "\n":
                current.append(" ")
                index += 1
            continue
        if character in {'"', "`"}:
            current.append(" ")
            quote_character = character
            index += 1
            continue
        current.append(character)
        index += 1
    output.append("".join(current))
    return output


def source_definition_line(source: str, source_path: str, anchor: str) -> int | None:
    """Return the sole real definition line for a planned test anchor."""

    suffix = Path(source_path).suffix.lower()
    if suffix == ".py":
        try:
            tree = ast.parse(source)
        except SyntaxError:
            return None
        matches = [
            node.lineno
            for node in ast.walk(tree)
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and node.name == anchor
        ]
    else:
        pattern_template = SOURCE_DEFINITION_PATTERNS.get(suffix)
        if pattern_template is None:
            return None
        code_lines = (
            c_like_code_lines(source)
            if suffix in {".rs", ".js", ".jsx", ".ts", ".tsx"}
            else source.splitlines()
        )
        pattern = re.compile(pattern_template.format(anchor=re.escape(anchor)))
        matches = [
            line_number
            for line_number, line in enumerate(code_lines, start=1)
            if not (
                suffix in {".ps1", ".sh"} and line.lstrip().startswith("#")
            )
            and pattern.search(line)
        ]
    return matches[0] if len(matches) == 1 else None


def has_unrendered_newline_escapes(text: str) -> bool:
    """Detect literal newline escapes outside fenced and inline code."""

    escaped_newlines = 0
    fence: str | None = None
    for line in (text or "").splitlines():
        stripped = line.lstrip()
        if fence:
            if stripped.startswith(fence):
                fence = None
            continue
        if stripped.startswith("```"):
            fence = "```"
            continue
        if stripped.startswith("~~~"):
            fence = "~~~"
            continue
        escaped_newlines += INLINE_CODE_RE.sub("", line).count(r"\n")
    return escaped_newlines >= 1


def issue_formatting_failures(
    issue: dict[str, object],
    comments: list[dict[str, object]],
    issue_number: int,
) -> list[str]:
    """Reject transport-escaped prose in issue bodies and trusted comments."""

    failures: list[str] = []
    if has_unrendered_newline_escapes(str(issue.get("body") or "")):
        failures.append(f"#{issue_number} body contains unrendered newline escapes")
    for comment in comments:
        association = comment.get("author_association", comment.get("authorAssociation"))
        if association not in TRUSTED_COMMENT_ASSOCIATIONS:
            continue
        if has_unrendered_newline_escapes(str(comment.get("body") or "")):
            location = (
                comment.get("html_url")
                or comment.get("url")
                or comment.get("id")
                or "unknown comment"
            )
            failures.append(
                f"#{issue_number} trusted comment {location} contains unrendered newline escapes"
            )
    return failures


def test_ids_from_text(text: str) -> tuple[str, ...]:
    """Extract both supported task-specific test identifier spellings."""

    found: list[str] = []
    for match in BACKTICK_TEST_ID_RE.finditer(text):
        found.append(match.group(1))
    for match in TAG_TEST_ID_RE.finditer(text):
        found.append(f"UT:{match.group(1)}")
    if len(found) != len(set(found)):
        raise ValidationError(f"task repeats a test identifier: {text}")
    return tuple(found)


def parse_tasks(text: str, *, require_test_ids: bool, source: str) -> list[Task]:
    """Parse strict canonical task rows in source order."""

    tasks: list[Task] = []
    section = ""
    seen_tasks: set[str] = set()
    for line_number, line in enumerate(normalize_newlines(text).split("\n"), start=1):
        heading = HEADING_RE.match(line)
        if heading and NUMBERED_HEADING_RE.match(heading.group(2)):
            section = heading.group(2).split(maxsplit=1)[0].rstrip(".")
        if not line.startswith("- ["):
            continue
        match = TASK_LINE_RE.match(line)
        if not match:
            raise ValidationError(f"{source}:{line_number} has a non-canonical task row")
        checked = match.group(1).lower() == "x"
        task_id = match.group(2)
        text_value = f"{task_id} {match.group(3)}"
        if task_id in seen_tasks:
            raise ValidationError(f"{source} duplicates task {task_id}")
        seen_tasks.add(task_id)
        test_ids = test_ids_from_text(text_value)
        if require_test_ids and not test_ids:
            raise ValidationError(f"{source} task {task_id} has no test identifier")
        if not section:
            raise ValidationError(f"{source} task {task_id} has no numbered section")
        tasks.append(Task(checked, task_id, text_value, test_ids, section))
    if not tasks:
        raise ValidationError(f"{source} has no canonical checkbox tasks")
    return tasks


def extract_task_section(body: str, source: str) -> str:
    """Extract the sole authoritative task section from an issue body."""

    normalized = normalize_newlines(body)
    lines = normalized.split("\n")
    matches: list[tuple[int, int]] = []
    for index, line in enumerate(lines):
        heading = HEADING_RE.match(line)
        if heading and heading.group(2).strip().lower() in TASK_HEADING_NAMES:
            matches.append((index, len(heading.group(1))))
    if len(matches) != 1:
        raise ValidationError(
            f"{source} must contain exactly one OpenSpec task checklist heading"
        )
    start, level = matches[0]
    end = len(lines)
    for index in range(start + 1, len(lines)):
        heading = HEADING_RE.match(lines[index])
        if not heading:
            continue
        if len(heading.group(1)) <= level and not NUMBERED_HEADING_RE.match(
            heading.group(2)
        ):
            end = index
            break
    return "\n".join(lines[start + 1 : end])


def local_task_changes(root: Path) -> set[str]:
    """Return every local OpenSpec change that owns a task file."""

    changes_dir = root / "openspec" / "changes"
    return {
        child.name
        for child in changes_dir.iterdir()
        if child.is_dir() and (child / "tasks.md").is_file()
    }


def local_tasks(root: Path, change: str, contract: str) -> tuple[Path, list[Task]]:
    """Load one local task file under its versioned evidence contract."""

    path = root / "openspec" / "changes" / change / "tasks.md"
    if not path.is_file():
        raise ValidationError(f"OpenSpec tasks file missing for {change}: {path}")
    return path, parse_tasks(
        path.read_text(encoding="utf-8"),
        require_test_ids=contract == "evidence-v2",
        source=str(path),
    )


def validate_owner_ranges(change: str, mapping: ChangeMapping, tasks: list[Task]) -> None:
    """Require ordered, complete, disjoint, single-owner task ranges."""

    index = {task.task_id: position for position, task in enumerate(tasks)}
    spans: list[tuple[int, int, Owner]] = []
    seen_issues: set[int] = set()
    for owner in mapping.owners:
        if owner.issue in seen_issues:
            raise ValidationError(f"{change} repeats authoritative issue #{owner.issue}")
        seen_issues.add(owner.issue)
        if owner.first_task not in index or owner.last_task not in index:
            raise ValidationError(f"{change} owner range references an unknown task")
        first = index[owner.first_task]
        last = index[owner.last_task]
        if first > last:
            raise ValidationError(f"{change} owner range is reversed")
        spans.append((first, last, owner))
    ordered = sorted(spans, key=lambda item: item[0])
    if spans != ordered:
        raise ValidationError(f"{change} owner ranges are reordered")
    expected = 0
    for first, last, _ in ordered:
        if first != expected:
            relation = "overlaps" if first < expected else "has a gap before"
            raise ValidationError(f"{change} owner range {relation} task {tasks[expected].task_id}")
        expected = last + 1
    if expected != len(tasks):
        raise ValidationError(f"{change} owner ranges omit task {tasks[expected].task_id}")
    if len(mapping.owners) == 1:
        owner = mapping.owners[0]
        if owner.issue != mapping.primary_issue:
            raise ValidationError(f"{change} single authority must be its primary issue")


def load_issue_map(
    path: Path, root: Path
) -> tuple[dict[str, ChangeMapping], dict[str, list[Task]]]:
    """Load schema-v2 mappings and validate every local authority range."""

    payload = require_object(json.loads(path.read_text(encoding="utf-8")), str(path))
    require_exact_keys(payload, {"schema_version", "changes"}, str(path))
    if payload["schema_version"] != SCHEMA_VERSION:
        raise ValidationError(f"{path} schema_version must be {SCHEMA_VERSION}")
    changes_value = require_object(payload["changes"], f"{path}.changes")
    local_changes = local_task_changes(root)
    mapped_changes = set(changes_value)
    if local_changes != mapped_changes:
        missing = sorted(local_changes - mapped_changes)
        extra = sorted(mapped_changes - local_changes)
        raise ValidationError(f"{path} mapping mismatch; missing={missing}, extra={extra}")
    mappings: dict[str, ChangeMapping] = {}
    task_sets: dict[str, list[Task]] = {}
    globally_owned_issues: dict[int, str] = {}
    global_test_ids: dict[str, str] = {}
    for change in sorted(changes_value):
        raw = require_object(changes_value[change], f"{path}.changes.{change}")
        require_exact_keys(raw, {"contract", "primary_issue", "owners"}, change)
        contract = require_string(raw["contract"], f"{change}.contract")
        if contract not in {"checklist-v1", "evidence-v2"}:
            raise ValidationError(f"{change}.contract is unsupported")
        primary_issue = require_positive_int(raw["primary_issue"], f"{change}.primary_issue")
        owners: list[Owner] = []
        for position, owner_value in enumerate(
            require_array(raw["owners"], f"{change}.owners")
        ):
            owner_raw = require_object(owner_value, f"{change}.owners[{position}]")
            require_exact_keys(
                owner_raw, {"issue", "first_task", "last_task"}, f"{change}.owners[{position}]"
            )
            first_task = require_string(owner_raw["first_task"], "first_task")
            last_task = require_string(owner_raw["last_task"], "last_task")
            if not TASK_ID_RE.fullmatch(first_task) or not TASK_ID_RE.fullmatch(last_task):
                raise ValidationError(f"{change} has a malformed owner task id")
            owners.append(
                Owner(
                    require_positive_int(owner_raw["issue"], "owner.issue"),
                    first_task,
                    last_task,
                )
            )
        if not owners:
            raise ValidationError(f"{change} has no authoritative owners")
        mapping = ChangeMapping(contract, primary_issue, tuple(owners))
        _, tasks = local_tasks(root, change, contract)
        validate_owner_ranges(change, mapping, tasks)
        for owner in owners:
            previous = globally_owned_issues.setdefault(owner.issue, change)
            if previous != change:
                raise ValidationError(
                    f"issue #{owner.issue} owns tasks for both {previous} and {change}"
                )
        if contract == "evidence-v2":
            for task in tasks:
                for test_id in task.test_ids:
                    previous = global_test_ids.setdefault(test_id, f"{change}/{task.task_id}")
                    if previous != f"{change}/{task.task_id}":
                        raise ValidationError(
                            f"test id {test_id} is reused by {previous} and {change}/{task.task_id}"
                        )
        mappings[change] = mapping
        task_sets[change] = tasks
    return mappings, task_sets


def normalized_relative_path(value: object, name: str) -> str:
    """Validate a canonical repository-relative slash-separated path."""

    path = require_string(value, name)
    pure = PurePosixPath(path)
    if (
        pure.is_absolute()
        or "\\" in path
        or path != pure.as_posix()
        or any(part in {"", ".", ".."} for part in pure.parts)
    ):
        raise ValidationError(f"{name} must be a normalized repository-relative path")
    return path


def validate_command(value: object, name: str) -> dict[str, object]:
    """Validate a bounded executable plus argument-array command."""

    command = require_object(value, name)
    require_exact_keys(command, {"executable", "arguments"}, name)
    executable = require_string(command["executable"], f"{name}.executable")
    if any(character.isspace() for character in executable):
        raise ValidationError(f"{name}.executable must not be a shell command string")
    arguments = require_array(command["arguments"], f"{name}.arguments")
    if len(arguments) > 64:
        raise ValidationError(f"{name}.arguments exceeds the bounded argument count")
    for position, argument in enumerate(arguments):
        text = require_string(argument, f"{name}.arguments[{position}]")
        if len(text) > 1024:
            raise ValidationError(f"{name}.arguments[{position}] is too long")
    return command


def load_verification_plan(
    path: Path,
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
) -> tuple[dict[tuple[str, str], dict[str, object]], dict[str, object]]:
    """Load the strict task verification plan for all evidence-v2 changes."""

    payload = require_object(json.loads(path.read_text(encoding="utf-8")), str(path))
    require_exact_keys(payload, {"schema_version", "changes"}, str(path))
    if payload["schema_version"] != PLAN_SCHEMA_VERSION:
        raise ValidationError(f"{path} has an unsupported schema version")
    changes = require_object(payload["changes"], f"{path}.changes")
    expected_changes = {
        change for change, mapping in mappings.items() if mapping.contract == "evidence-v2"
    }
    if set(changes) != expected_changes:
        raise ValidationError(
            f"{path} evidence-v2 changes mismatch; expected {sorted(expected_changes)}"
        )
    index: dict[tuple[str, str], dict[str, object]] = {}
    for change in sorted(changes):
        raw_change = require_object(changes[change], f"{change} plan")
        require_exact_keys(raw_change, {"tasks"}, f"{change} plan")
        raw_tasks = require_array(raw_change["tasks"], f"{change}.tasks")
        expected_tasks = task_sets[change]
        if len(raw_tasks) != len(expected_tasks):
            raise ValidationError(f"{change} plan task count does not match tasks.md")
        for position, (raw_value, expected) in enumerate(zip(raw_tasks, expected_tasks)):
            raw = require_object(raw_value, f"{change}.tasks[{position}]")
            required_keys = {
                "task_id",
                "test_ids",
                "assertion",
                "command",
                "timeout_seconds",
                "covered_inputs",
            }
            if not required_keys <= set(raw) or set(raw) - required_keys not in (
                set(),
                {"test_sources"},
            ):
                raise ValidationError(
                    f"{change}.tasks[{position}] has unexpected keys: "
                    f"{sorted(set(raw) - required_keys - {'test_sources'})}"
                )
            task_id = require_string(raw["task_id"], "task_id")
            if task_id != expected.task_id:
                raise ValidationError(
                    f"{change} plan is reordered at {task_id}; expected {expected.task_id}"
                )
            test_ids = tuple(
                require_string(value, f"{change}/{task_id}.test_ids")
                for value in require_array(raw["test_ids"], "test_ids")
            )
            if test_ids != expected.test_ids:
                raise ValidationError(f"{change}/{task_id} plan test ids do not match tasks.md")
            test_sources = require_array(raw.get("test_sources", []), "test_sources")
            source_ids: list[str] = []
            for source_position, source_value in enumerate(test_sources):
                source = require_object(
                    source_value,
                    f"{change}/{task_id}.test_sources[{source_position}]",
                )
                require_exact_keys(
                    source,
                    {"test_id", "path", "anchor"},
                    f"{change}/{task_id}.test_sources[{source_position}]",
                )
                source_test_id = require_string(source["test_id"], "test_source.test_id")
                source_path = normalized_relative_path(source["path"], "test_source.path")
                source_anchor = require_string(source["anchor"], "test_source.anchor")
                if Path(source_path).suffix.lower() not in SOURCE_DEFINITION_PATTERNS:
                    raise ValidationError(
                        f"{change}/{task_id} has an unsupported test source suffix"
                    )
                if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", source_anchor):
                    raise ValidationError(
                        f"{change}/{task_id} has a malformed test source anchor"
                    )
                source_ids.append(source_test_id)
            if source_ids and tuple(source_ids) != test_ids:
                raise ValidationError(
                    f"{change}/{task_id} test sources do not match its test ids"
                )
            assertion = require_string(raw["assertion"], f"{change}/{task_id}.assertion")
            if len(assertion) > 4096 or any(ord(character) < 32 for character in assertion):
                raise ValidationError(f"{change}/{task_id} assertion is not bounded plain text")
            validate_command(raw["command"], f"{change}/{task_id}.command")
            timeout = require_positive_int(raw["timeout_seconds"], "timeout_seconds")
            if timeout > 86_400:
                raise ValidationError(f"{change}/{task_id} timeout is unbounded")
            covered = require_array(raw["covered_inputs"], "covered_inputs")
            if not covered:
                raise ValidationError(f"{change}/{task_id} has no covered inputs")
            seen_paths: set[tuple[str, str]] = set()
            covered_paths: set[str] = set()
            for input_position, input_value in enumerate(covered):
                covered_input = require_object(
                    input_value, f"{change}/{task_id}.covered_inputs[{input_position}]"
                )
                require_exact_keys(covered_input, {"kind", "path"}, "covered_input")
                kind = require_string(covered_input["kind"], "covered_input.kind")
                if kind not in {"file", "tree"}:
                    raise ValidationError(f"{change}/{task_id} has an unsupported input kind")
                input_path = normalized_relative_path(
                    covered_input["path"], "covered_input.path"
                )
                identity = (kind, input_path)
                if identity in seen_paths:
                    raise ValidationError(f"{change}/{task_id} duplicates covered input {identity}")
                seen_paths.add(identity)
                covered_paths.add(input_path)
            if change == TQG_CHANGE:
                required_paths = TQG_REQUIRED_SECTION_INPUTS.get(expected.section)
                if required_paths is None:
                    raise ValidationError(f"{change}/{task_id} has an unknown section")
                missing_paths = required_paths - covered_paths
                if missing_paths:
                    raise ValidationError(
                        f"{change}/{task_id} has incomplete covered inputs: "
                        f"{', '.join(sorted(missing_paths))}"
                    )
            index[(change, task_id)] = raw
    return index, payload


def canonical_json(value: object) -> bytes:
    """Return deterministic UTF-8 JSON for digests and equality checks."""

    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")


def sha256_digest(value: bytes) -> str:
    """Return a normalized SHA-256 identity."""

    return f"sha256:{hashlib.sha256(value).hexdigest()}"


def normalized_covered_value(value: bytes, relative: str) -> bytes:
    """Normalize covered bytes without hiding implementation changes."""

    if relative.endswith("/tasks.md"):
        text = normalize_newlines(value.decode("utf-8"))
        text = re.sub(r"(?m)^- \[[xX]\] ", "- [ ] ", text)
        return text.encode("utf-8")
    if relative == "openspec/task-verification.json":
        # The canonical current task plan row is already part of the digest.
        return b"task-plan-entry-normalized-by-covered-input-digest-v1"
    if relative == "openspec/task-evidence.json":
        # Evidence pointers are closure metadata validated by semantic diff.
        return b"task-evidence-metadata-normalized-by-issueops-v1"
    try:
        text = value.decode("utf-8")
    except UnicodeDecodeError:
        return value
    return normalize_newlines(text).encode("utf-8")


def normalized_covered_bytes(path: Path, relative: str) -> bytes:
    """Read one covered input and apply its narrow metadata normalization."""

    return normalized_covered_value(path.read_bytes(), relative)


def covered_input_digest(root: Path, plan_row: dict[str, object]) -> str:
    """Digest a canonical plan row and its sorted, root-confined inputs."""

    root_resolved = root.resolve()
    entries: list[tuple[str, bytes]] = []
    covered = require_array(plan_row["covered_inputs"], "covered_inputs")
    for value in covered:
        item = require_object(value, "covered_input")
        relative = normalized_relative_path(item["path"], "covered_input.path")
        candidate = root / PurePosixPath(relative)
        if item["kind"] == "file":
            paths = [candidate]
        else:
            if not candidate.is_dir():
                raise ValidationError(f"covered tree is missing: {relative}")
            paths = sorted(path for path in candidate.rglob("*") if path.is_file())
            if not paths:
                raise ValidationError(f"covered tree is empty: {relative}")
        for path in paths:
            if path.is_symlink():
                raise ValidationError(f"covered input must not be a symlink: {path}")
            resolved = path.resolve()
            try:
                resolved.relative_to(root_resolved)
            except ValueError as error:
                raise ValidationError(f"covered input escapes the repository: {path}") from error
            relative_file = resolved.relative_to(root_resolved).as_posix()
            entries.append(
                (relative_file, normalized_covered_bytes(resolved, relative_file))
            )
    digest = hashlib.sha256()
    plan_projection = {
        key: plan_row[key]
        for key in (
            "task_id",
            "test_ids",
            "assertion",
            "command",
            "timeout_seconds",
            "covered_inputs",
        )
    }
    digest.update(canonical_json(plan_projection))
    digest.update(b"\0")
    for relative, value in sorted(entries):
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(hashlib.sha256(value).digest())
        digest.update(b"\n")
    return f"sha256:{digest.hexdigest()}"


def parse_timestamp(value: object, name: str) -> datetime:
    """Parse one timezone-bearing RFC3339 timestamp."""

    text = require_string(value, name)
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError as error:
        raise ValidationError(f"{name} must be an RFC3339 timestamp") from error
    if parsed.tzinfo is None:
        raise ValidationError(f"{name} must include a timezone")
    return parsed


def validate_retained_result(
    value: object, name: str, root: Path | None = None
) -> dict[str, object]:
    """Validate retained local or GitHub run provenance without trusting URLs."""

    result = require_object(value, name)
    kind = require_string(result.get("kind"), f"{name}.kind")
    if kind == "github_actions":
        required = {
            "kind",
            "repository",
            "run_id",
            "run_attempt",
            "job_id",
            "job_name",
            "artifact_id",
            "artifact_name",
            "artifact_digest",
            "result_path",
            "result_digest",
        }
        require_exact_keys(result, required, name)
        repo_parts(require_string(result["repository"], f"{name}.repository"))
        for key in ("run_id", "run_attempt", "job_id", "artifact_id"):
            require_positive_int(result[key], f"{name}.{key}")
        for key in ("job_name", "artifact_name"):
            require_string(result[key], f"{name}.{key}")
        normalized_relative_path(result["result_path"], f"{name}.result_path")
        for key in ("artifact_digest", "result_digest"):
            digest = require_string(result[key], f"{name}.{key}")
            if not DIGEST_RE.fullmatch(digest):
                raise ValidationError(f"{name}.{key} is not a canonical digest")
    elif kind == "repository":
        require_exact_keys(result, {"kind", "result_path", "result_digest"}, name)
        result_path = normalized_relative_path(
            result["result_path"], f"{name}.result_path"
        )
        result_digest = require_string(result["result_digest"], "result_digest")
        if not DIGEST_RE.fullmatch(result_digest):
            raise ValidationError(f"{name}.result_digest is not canonical")
        if root is not None:
            candidate = root / PurePosixPath(result_path)
            if candidate.is_symlink() or not candidate.is_file():
                raise ValidationError(f"{name} repository result is missing: {result_path}")
            if sha256_digest(candidate.read_bytes()) != result_digest:
                raise ValidationError(f"{name} repository result digest differs: {result_path}")
    else:
        raise ValidationError(f"{name}.kind is unsupported")
    return result


def load_evidence(
    path: Path,
    root: Path,
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
    plan: dict[tuple[str, str], dict[str, object]],
) -> tuple[dict[tuple[str, str, str], dict[str, object]], dict[str, object]]:
    """Load and validate the current task-evidence ledger."""

    payload = require_object(json.loads(path.read_text(encoding="utf-8")), str(path))
    require_exact_keys(payload, {"schema_version", "results"}, str(path))
    if payload["schema_version"] != EVIDENCE_SCHEMA_VERSION:
        raise ValidationError(f"{path} has an unsupported schema version")
    task_lookup = {
        (change, task.task_id): task for change, tasks in task_sets.items() for task in tasks
    }
    results: dict[tuple[str, str, str], dict[str, object]] = {}
    commit_cache: dict[str, dict[object, object]] = {}
    for position, value in enumerate(require_array(payload["results"], "results")):
        row = require_object(value, f"results[{position}]")
        require_exact_keys(
            row,
            {
                "change",
                "task_id",
                "test_id",
                "outcome",
                "tested_commit",
                "covered_input_digest",
                "platform",
                "started_at",
                "completed_at",
                "retained_result",
            },
            f"results[{position}]",
        )
        change = require_string(row["change"], "change")
        task_id = require_string(row["task_id"], "task_id")
        test_id = require_string(row["test_id"], "test_id")
        task = task_lookup.get((change, task_id))
        if not task or mappings[change].contract != "evidence-v2":
            raise ValidationError(f"evidence row is orphaned: {change}/{task_id}")
        if test_id not in task.test_ids:
            raise ValidationError(f"evidence names the wrong test for {change}/{task_id}")
        identity = (change, task_id, test_id)
        if identity in results:
            raise ValidationError(f"evidence duplicates {change}/{task_id}/{test_id}")
        if row["outcome"] != "passed":
            raise ValidationError(f"current evidence must be passing: {identity}")
        tested_commit = require_string(row["tested_commit"], "tested_commit")
        if not SHA_RE.fullmatch(tested_commit):
            raise ValidationError(f"{identity} has a non-canonical tested commit")
        validate_tested_commit(
            root,
            change,
            task_id,
            tested_commit,
            plan[(change, task_id)],
            commit_cache,
        )
        digest = require_string(row["covered_input_digest"], "covered_input_digest")
        if not DIGEST_RE.fullmatch(digest):
            raise ValidationError(f"{identity} has a non-canonical covered-input digest")
        if digest != covered_input_digest(root, plan[(change, task_id)]):
            raise ValidationError(f"{identity} is stale for its covered inputs")
        platform = row["platform"]
        if platform is not None:
            platform_value = require_object(platform, "platform")
            require_exact_keys(platform_value, {"os", "arch"}, "platform")
            require_string(platform_value["os"], "platform.os")
            require_string(platform_value["arch"], "platform.arch")
        started = parse_timestamp(row["started_at"], "started_at")
        completed = parse_timestamp(row["completed_at"], "completed_at")
        if completed < started:
            raise ValidationError(f"{identity} completes before it starts")
        validate_retained_result(row["retained_result"], "retained_result", root)
        results[identity] = row
    return results, payload


def checked_evidence_failures(
    change: str,
    tasks: list[Task],
    evidence: dict[tuple[str, str, str], dict[str, object]],
    only_task_ids: set[str] | None = None,
) -> list[str]:
    """Return missing evidence failures for checked evidence-v2 tasks."""

    failures: list[str] = []
    for task in tasks:
        if only_task_ids is not None and task.task_id not in only_task_ids:
            continue
        if not task.checked:
            if only_task_ids is not None:
                failures.append(f"{change}/{task.task_id} is in PR scope but is not complete")
            continue
        for test_id in task.test_ids:
            if (change, task.task_id, test_id) not in evidence:
                failures.append(f"{change}/{task.task_id} lacks current evidence for {test_id}")
    return failures


def repo_parts(repo: str) -> tuple[str, str]:
    """Validate and split an OWNER/REPO identity."""

    if not REPOSITORY_RE.fullmatch(repo):
        raise ValidationError(f"--repo must be OWNER/REPO, got {repo!r}")
    parts = repo.split("/", 1)
    return parts[0], parts[1]


def flatten_paginated_response(payload: object) -> list[object]:
    """Flatten `gh api --paginate --slurp` output."""

    pages = require_array(payload, "GitHub pagination response")
    if all(isinstance(page, list) for page in pages):
        return [item for page in pages for item in page]
    if any(isinstance(page, list) for page in pages):
        raise ValidationError("GitHub pagination response mixes pages and items")
    return pages


def issue_payload(repo: str, number: int) -> dict[str, object]:
    """Fetch one issue directly from the GitHub API."""

    owner, name = repo_parts(repo)
    return require_object(
        gh_api_json([f"repos/{owner}/{name}/issues/{number}"]), f"issue #{number}"
    )


def issue_comments(repo: str, number: int) -> list[dict[str, object]]:
    """Fetch every issue comment through paginated GitHub API calls."""

    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            "--method",
            "GET",
            f"repos/{owner}/{name}/issues/{number}/comments",
            "-F",
            "per_page=100",
        ]
    )
    return [require_object(value, "issue comment") for value in flatten_paginated_response(payload)]


def issue_tasks(issue: dict[str, object], contract: str, number: int) -> list[Task]:
    """Parse one issue's sole authoritative checklist."""

    body = issue.get("body") or ""
    if not isinstance(body, str):
        raise ValidationError(f"issue #{number} body is not text")
    return parse_tasks(
        extract_task_section(body, f"issue #{number}"),
        require_test_ids=contract == "evidence-v2",
        source=f"issue #{number}",
    )


def task_slice(tasks: list[Task], owner: Owner) -> list[Task]:
    """Return one inclusive owner slice from canonical local order."""

    index = {task.task_id: position for position, task in enumerate(tasks)}
    return tasks[index[owner.first_task] : index[owner.last_task] + 1]


def compare_task_sequences(
    expected: list[Task], actual: list[Task], source: str
) -> list[str]:
    """Compare exact task identity, text, section, IDs, order, and state."""

    if expected == actual:
        return []
    failures: list[str] = []
    limit = max(len(expected), len(actual))
    for position in range(limit):
        left = expected[position] if position < len(expected) else None
        right = actual[position] if position < len(actual) else None
        if left != right:
            failures.append(
                f"{source} task sequence differs at position {position + 1}: "
                f"expected={left}, actual={right}"
            )
            break
    if len(expected) != len(actual):
        failures.append(
            f"{source} task count differs: expected {len(expected)}, actual {len(actual)}"
        )
    return failures


def hypothetical_primary_length(body: str, tasks_path: Path) -> int:
    """Measure the primary issue with its complete canonical local checklist."""

    normalized = normalize_newlines(body)
    marker_index = normalized.find("\n<!-- projectatlas-task-owners:v1")
    checklist_match = re.search(
        r"(?m)^## (?:OpenSpec Tasks|OpenSpec Task Checklist)\s*$", normalized
    )
    cuts = [value for value in (marker_index, checklist_match.start() if checklist_match else -1) if value >= 0]
    prefix = normalized[: min(cuts)] if cuts else normalized
    complete = (
        prefix.rstrip()
        + "\n\n## OpenSpec Task Checklist\n\n"
        + normalize_newlines(tasks_path.read_text(encoding="utf-8")).strip()
        + "\n"
    )
    return len(complete)


def check_openspec_tasks(
    repo: str,
    root: Path,
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
    evidence: dict[tuple[str, str, str], dict[str, object]],
    selected_changes: set[str] | None = None,
) -> list[str]:
    """Validate exact local/remote authority for selected or all changes."""

    failures: list[str] = []
    checked_issue_formatting: set[int] = set()
    comment_cache: dict[int, list[dict[str, object]]] = {}
    for change in sorted(mappings):
        if selected_changes is not None and change not in selected_changes:
            continue
        mapping = mappings[change]
        tasks = task_sets[change]
        path = root / "openspec" / "changes" / change / "tasks.md"
        primary = issue_payload(repo, mapping.primary_issue)
        primary_body = primary.get("body") or ""
        if not isinstance(primary_body, str):
            failures.append(f"#{mapping.primary_issue} has a non-text body")
            continue
        if len(primary_body) > BODY_CHARACTER_LIMIT:
            failures.append(f"#{mapping.primary_issue} exceeds GitHub's body limit")
        primary_comments = comment_cache.setdefault(
            mapping.primary_issue,
            issue_comments(repo, mapping.primary_issue),
        )
        failures.extend(
            issue_formatting_failures(
                primary, primary_comments, mapping.primary_issue
            )
        )
        checked_issue_formatting.add(mapping.primary_issue)
        if len(mapping.owners) > 1 and hypothetical_primary_length(primary_body, path) <= BODY_CHARACTER_LIMIT:
            failures.append(f"{change} uses phase issues even though one primary body fits")
        checked_total = 0
        for owner in mapping.owners:
            issue = primary if owner.issue == mapping.primary_issue else issue_payload(repo, owner.issue)
            if owner.issue not in checked_issue_formatting:
                comments = comment_cache.setdefault(
                    owner.issue, issue_comments(repo, owner.issue)
                )
                failures.extend(
                    issue_formatting_failures(issue, comments, owner.issue)
                )
                checked_issue_formatting.add(owner.issue)
            body = issue.get("body") or ""
            if not isinstance(body, str) or len(body) > BODY_CHARACTER_LIMIT:
                failures.append(f"#{owner.issue} has an invalid or oversized body")
                continue
            try:
                remote = issue_tasks(issue, mapping.contract, owner.issue)
            except ValidationError as error:
                failures.append(str(error))
                continue
            expected = task_slice(tasks, owner)
            failures.extend(compare_task_sequences(expected, remote, f"#{owner.issue} {change}"))
            checked_total += sum(task.checked for task in remote)
            if str(issue.get("state", "")).lower() == "closed" and any(
                not task.checked for task in remote
            ):
                failures.append(f"#{owner.issue} is closed but has unchecked tasks")
        if mapping.contract == "evidence-v2":
            failures.extend(checked_evidence_failures(change, tasks, evidence))
        print(
            f"#{mapping.primary_issue} {change}: local {len(tasks)} / "
            f"remote checked {checked_total} / owners {len(mapping.owners)}"
        )
    return failures


def milestone_number(repo: str, milestone: str) -> int | None:
    """Resolve an exact milestone title across all GitHub pages."""

    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            "--method",
            "GET",
            f"repos/{owner}/{name}/milestones",
            "-F",
            "state=all",
            "-F",
            "per_page=100",
        ]
    )
    matches = [
        require_object(item, "milestone")
        for item in flatten_paginated_response(payload)
        if require_object(item, "milestone").get("title") == milestone
    ]
    if len(matches) > 1:
        raise ValidationError(f"milestone title is ambiguous: {milestone}")
    return int(matches[0]["number"]) if matches else None


def milestone_issues(repo: str, milestone: str) -> list[dict[str, object]]:
    """Return every non-PR issue in one exact milestone."""

    number = milestone_number(repo, milestone)
    if number is None:
        return []
    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            "--method",
            "GET",
            f"repos/{owner}/{name}/issues",
            "-F",
            "state=all",
            "-F",
            f"milestone={number}",
            "-F",
            "per_page=100",
        ]
    )
    return [
        require_object(item, "milestone issue")
        for item in flatten_paginated_response(payload)
        if "pull_request" not in require_object(item, "milestone issue")
    ]


def check_milestone_complete(
    repo: str,
    milestone: str,
    root: Path,
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
    evidence: dict[tuple[str, str, str], dict[str, object]],
) -> list[str]:
    """Apply the stronger full-release issue, task, and evidence gate."""

    failures: list[str] = []
    issues = milestone_issues(repo, milestone)
    if not issues:
        return [f"milestone {milestone!r} has no issues"]
    issue_to_change = {
        owner.issue: change for change, mapping in mappings.items() for owner in mapping.owners
    }
    selected_changes: set[str] = set()
    for issue in issues:
        number = require_positive_int(issue.get("number"), "milestone issue number")
        change = issue_to_change.get(number)
        if not change:
            failures.append(f"#{number} in milestone {milestone} has no authoritative map entry")
            continue
        selected_changes.add(change)
        if str(issue.get("state", "")).lower() != "closed":
            failures.append(f"#{number} in milestone {milestone} is still open")
    failures.extend(
        check_openspec_tasks(
            repo, root=root, mappings=mappings, task_sets=task_sets,
            evidence=evidence, selected_changes=selected_changes
        )
    )
    for change in selected_changes:
        tasks = task_sets[change]
        if any(not task.checked for task in tasks):
            failures.append(f"{change} has incomplete release tasks")
        if mappings[change].contract == "evidence-v2":
            failures.extend(checked_evidence_failures(change, tasks, evidence))
    return failures


def parse_pr_scope(body: str) -> tuple[set[int], list[ScopeRow]]:
    """Parse exact issue and task scope declarations from a PR body."""

    issues = {int(value) for value in PR_ISSUE_RE.findall(body or "")}
    rows = [ScopeRow(change, first, last or first) for change, first, last in PR_TASK_RE.findall(body or "")]
    if not issues:
        raise ValidationError("pull request has no OpenSpec-Issue declaration")
    if not rows:
        raise ValidationError("pull request has no OpenSpec-Task declaration")
    return issues, rows


def expand_pr_scope(
    declared_issues: set[int],
    rows: list[ScopeRow],
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
) -> dict[str, set[str]]:
    """Expand PR ranges and require unambiguous authoritative issue linkage."""

    expanded: dict[str, set[str]] = {}
    used_issues: set[int] = set()
    for row in rows:
        if row.change not in mappings:
            raise ValidationError(f"PR scope names unknown change {row.change}")
        tasks = task_sets[row.change]
        index = {task.task_id: position for position, task in enumerate(tasks)}
        if row.first_task not in index or row.last_task not in index:
            raise ValidationError(f"PR scope names an unknown task in {row.change}")
        first = index[row.first_task]
        last = index[row.last_task]
        if first > last:
            raise ValidationError(f"PR scope reverses a task range in {row.change}")
        selected = tasks[first : last + 1]
        owners = {
            owner.issue
            for owner in mappings[row.change].owners
            if any(task in task_slice(tasks, owner) for task in selected)
        }
        if len(owners) != 1:
            raise ValidationError(f"PR task range crosses authority boundaries: {row}")
        owner_issue = next(iter(owners))
        if owner_issue not in declared_issues:
            raise ValidationError(f"PR scope does not link authoritative issue #{owner_issue}")
        used_issues.add(owner_issue)
        target = expanded.setdefault(row.change, set())
        overlap = target.intersection(task.task_id for task in selected)
        if overlap:
            raise ValidationError(f"PR scope duplicates tasks: {sorted(overlap)}")
        target.update(task.task_id for task in selected)
    if used_issues != declared_issues:
        raise ValidationError(
            f"PR declares unused or ambiguous issues: {sorted(declared_issues - used_issues)}"
        )
    return expanded


def input_covers(path: str, covered_input: dict[str, object]) -> bool:
    """Return whether one plan selector owns a changed repository path."""

    target = normalized_relative_path(covered_input["path"], "covered_input.path")
    if covered_input["kind"] == "file":
        return path == target
    return path == target or path.startswith(f"{target}/")


def task_verification_receipt_path(path: str) -> bool:
    """Recognize a retained task-verification receipt below benchmark results."""

    if not path.startswith("docs/benchmarks/results/"):
        return False
    filename = path.rsplit("/", 1)[-1]
    return filename.startswith("task-verification-") and filename.lower().endswith(
        ".json"
    )


def semantic_metadata_path(path: str, scoped_changes: set[str]) -> bool:
    """Recognize metadata that still requires semantic scope validation."""

    if path in {"openspec/task-verification.json", "openspec/task-evidence.json"}:
        return True
    if task_verification_receipt_path(path):
        return True
    return any(path == f"openspec/changes/{change}/tasks.md" for change in scoped_changes)


def changed_paths_are_owned(
    changed_paths: list[str],
    scope: dict[str, set[str]],
    plan: dict[tuple[str, str], dict[str, object]],
) -> list[str]:
    """Reject files outside declared task ownership and semantic metadata."""

    failures: list[str] = []
    scoped_changes = set(scope)
    selectors = [
        require_object(value, "covered_input")
        for change, task_ids in scope.items()
        for task_id in task_ids
        for value in require_array(plan[(change, task_id)]["covered_inputs"], "covered_inputs")
    ]
    for path in changed_paths:
        normalized = normalized_relative_path(path, "changed path")
        if semantic_metadata_path(normalized, scoped_changes):
            continue
        if not any(input_covers(normalized, selector) for selector in selectors):
            failures.append(f"changed path is outside declared OpenSpec scope: {normalized}")
    return failures


def git_file_at(root: Path, commit: str, relative_path: str) -> str | None:
    """Read one repository file at a validated commit using a fixed git argv."""

    if not SHA_RE.fullmatch(commit):
        raise ValidationError("git snapshot commit is not canonical")
    path = normalized_relative_path(relative_path, "git snapshot path")
    process = subprocess.run(
        ["git", "-C", str(root), "show", f"{commit}:{path}"],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if process.returncode == 0:
        return process.stdout
    missing_fragments = (
        "does not exist in",
        "exists on disk, but not in",
        "Path '" + path + "' does not exist",
    )
    if any(fragment in process.stderr for fragment in missing_fragments):
        return None
    raise ValidationError(
        f"failed to read {path} at {commit}: {process.stderr.strip()}"
    )


def git_bytes_at(root: Path, commit: str, relative_path: str) -> bytes | None:
    """Read one repository blob at a validated commit without text decoding."""

    if not SHA_RE.fullmatch(commit):
        raise ValidationError("git snapshot commit is not canonical")
    path = normalized_relative_path(relative_path, "git snapshot path")
    process = subprocess.run(
        ["git", "-C", str(root), "show", f"{commit}:{path}"],
        capture_output=True,
        timeout=30,
        check=False,
    )
    if process.returncode == 0:
        return process.stdout
    error_text = process.stderr.decode("utf-8", errors="replace")
    missing_fragments = (
        "does not exist in",
        "exists on disk, but not in",
        "Path '" + path + "' does not exist",
    )
    if any(fragment in error_text for fragment in missing_fragments):
        return None
    raise ValidationError(f"failed to read {path} at {commit}: {error_text.strip()}")


def git_output(root: Path, arguments: list[str], description: str) -> str:
    """Run one fixed read-only Git command and return normalized output."""

    process = subprocess.run(
        ["git", "-C", str(root), *arguments],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if process.returncode:
        raise ValidationError(f"{description}: {process.stderr.strip()}")
    return normalize_newlines(process.stdout).strip()


def covered_entries_from_worktree(
    root: Path, covered_input: dict[str, object]
) -> dict[str, bytes]:
    """Read one validated covered selector from the current worktree."""

    relative = normalized_relative_path(covered_input["path"], "covered_input.path")
    candidate = root / PurePosixPath(relative)
    if covered_input["kind"] == "file":
        paths = [candidate]
    else:
        if not candidate.is_dir():
            raise ValidationError(f"covered tree is missing: {relative}")
        paths = sorted(path for path in candidate.rglob("*") if path.is_file())
        if not paths:
            raise ValidationError(f"covered tree is empty: {relative}")
    entries: dict[str, bytes] = {}
    root_resolved = root.resolve()
    for path in paths:
        if path.is_symlink():
            raise ValidationError(f"covered input must not be a symlink: {path}")
        resolved = path.resolve()
        try:
            relative_file = resolved.relative_to(root_resolved).as_posix()
        except ValueError as error:
            raise ValidationError(f"covered input escapes the repository: {path}") from error
        if not resolved.is_file():
            raise ValidationError(f"covered file is missing: {relative_file}")
        entries[relative_file] = normalized_covered_bytes(resolved, relative_file)
    return entries


def covered_entries_at_commit(
    root: Path, commit: str, covered_input: dict[str, object]
) -> dict[str, bytes]:
    """Read one covered selector from an immutable Git tree."""

    relative = normalized_relative_path(covered_input["path"], "covered_input.path")
    if covered_input["kind"] == "file":
        paths = [relative]
    else:
        output = git_output(
            root,
            ["ls-tree", "-r", "--name-only", commit, "--", relative],
            f"failed to enumerate covered tree {relative} at {commit}",
        )
        paths = [line for line in output.split("\n") if line]
        if not paths:
            raise ValidationError(f"covered tree is missing at tested commit: {relative}")
    entries: dict[str, bytes] = {}
    for path in paths:
        value = git_bytes_at(root, commit, path)
        if value is None:
            raise ValidationError(f"covered file is missing at tested commit: {path}")
        entries[path] = normalized_covered_value(value, path)
    return entries


def validate_tested_commit(
    root: Path,
    change: str,
    task_id: str,
    tested_commit: str,
    plan_row: dict[str, object],
    cache: dict[str, dict[object, object]] | None = None,
) -> None:
    """Bind evidence to HEAD or a semantics-preserving metadata-only ancestor."""

    cache = cache if cache is not None else {}
    repository = cache.setdefault("repository", {})
    if "head" not in repository:
        repository["head"] = git_output(
            root,
            ["rev-parse", "--verify", "HEAD^{commit}"],
            "cannot resolve HEAD",
        )
    head = require_string(repository["head"], "repository HEAD")
    if not SHA_RE.fullmatch(head):
        raise ValidationError("repository HEAD is not canonical")
    ancestry = cache.setdefault("ancestry", {})
    ancestry_key = (tested_commit, head)
    if ancestry_key not in ancestry:
        process = subprocess.run(
            ["git", "-C", str(root), "merge-base", "--is-ancestor", tested_commit, head],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        ancestry[ancestry_key] = process.returncode == 0
    if ancestry[ancestry_key] is not True:
        raise ValidationError(
            f"{change}/{task_id} tested commit is not an ancestor of current HEAD"
        )
    task_path = f"openspec/changes/{change}/tasks.md"
    metadata_paths = {
        task_path,
        "openspec/task-verification.json",
        "openspec/task-evidence.json",
    }

    tested_blobs = cache.setdefault("tested_blobs", {})
    task_blob_key = (tested_commit, task_path)
    if task_blob_key not in tested_blobs:
        tested_blobs[task_blob_key] = git_bytes_at(root, tested_commit, task_path)
    tested_tasks = tested_blobs[task_blob_key]
    current_values = cache.setdefault("current_values", {})
    if task_path not in current_values:
        current_tasks = root / PurePosixPath(task_path)
        current_values[task_path] = (
            normalized_covered_bytes(current_tasks, task_path)
            if current_tasks.is_file()
            else None
        )
    if (
        tested_tasks is None
        or normalized_covered_value(tested_tasks, task_path)
        != current_values[task_path]
    ):
        raise ValidationError(
            f"{change}/{task_id} task definitions differ from the tested commit"
        )

    tested_plans = cache.setdefault("tested_plans", {})
    if tested_commit not in tested_plans:
        tested_plan_text = git_file_at(
            root, tested_commit, "openspec/task-verification.json"
        )
        if tested_plan_text is None:
            raise ValidationError(
                f"{change}/{task_id} has no plan row at the tested commit"
            )
        try:
            tested_plans[tested_commit] = plan_rows_from_payload(
                json.loads(tested_plan_text)
            )
        except (json.JSONDecodeError, ValidationError) as error:
            raise ValidationError(
                f"{change}/{task_id} tested plan snapshot is invalid"
            ) from error
    tested_rows = require_object(tested_plans[tested_commit], "tested plan rows")
    if canonical_json(tested_rows.get((change, task_id))) != canonical_json(plan_row):
        raise ValidationError(f"{change}/{task_id} plan assertion differs from the tested commit")

    for value in require_array(plan_row["covered_inputs"], "covered_inputs"):
        covered_input = require_object(value, "covered_input")
        relative = normalized_relative_path(covered_input["path"], "covered_input.path")
        if relative in metadata_paths:
            continue
        selector = (covered_input["kind"], relative)
        tested_inputs = cache.setdefault("tested_inputs", {})
        tested_input_key = (tested_commit, *selector)
        if tested_input_key not in tested_inputs:
            tested_inputs[tested_input_key] = covered_entries_at_commit(
                root, tested_commit, covered_input
            )
        current_inputs = cache.setdefault("current_inputs", {})
        if selector not in current_inputs:
            current_inputs[selector] = covered_entries_from_worktree(
                root, covered_input
            )
        if tested_inputs[tested_input_key] != current_inputs[selector]:
            raise ValidationError(
                f"{change}/{task_id} covered input differs from the tested commit: {relative}"
            )


def plan_rows_from_payload(value: object) -> dict[tuple[str, str], object]:
    """Project a verification plan into change/task rows for semantic diffing."""

    if value is None:
        return {}
    payload = require_object(value, "verification plan snapshot")
    changes = require_object(payload.get("changes"), "verification plan snapshot.changes")
    rows: dict[tuple[str, str], object] = {}
    for change, change_value in changes.items():
        raw_change = require_object(change_value, f"verification plan {change}")
        for task_value in require_array(raw_change.get("tasks"), f"verification plan {change}.tasks"):
            task = require_object(task_value, f"verification plan {change} task")
            task_id = require_string(task.get("task_id"), "verification plan task_id")
            identity = (change, task_id)
            if identity in rows:
                raise ValidationError(f"verification plan snapshot duplicates {change}/{task_id}")
            rows[identity] = task
    return rows


def evidence_rows_from_payload(value: object) -> dict[tuple[str, str, str], object]:
    """Project an evidence ledger into task/test rows for semantic diffing."""

    if value is None:
        return {}
    payload = require_object(value, "evidence snapshot")
    rows: dict[tuple[str, str, str], object] = {}
    for result_value in require_array(payload.get("results"), "evidence snapshot.results"):
        result = require_object(result_value, "evidence result")
        identity = (
            require_string(result.get("change"), "evidence change"),
            require_string(result.get("task_id"), "evidence task_id"),
            require_string(result.get("test_id"), "evidence test_id"),
        )
        if identity in rows:
            raise ValidationError(f"evidence snapshot duplicates {identity}")
        rows[identity] = result
    return rows


def changed_mapping_keys(before: dict[object, object], after: dict[object, object]) -> set[object]:
    """Return keys added, removed, or changed between canonical maps."""

    return {
        key
        for key in before.keys() | after.keys()
        if canonical_json(before.get(key)) != canonical_json(after.get(key))
    }


def validate_task_metadata_transition(
    change: str,
    before_text: str | None,
    current_tasks: list[Task],
    scoped_task_ids: set[str],
    require_test_ids: bool,
) -> list[str]:
    """Allow only scoped unchecked-to-checked task metadata transitions."""

    if before_text is None:
        return [f"{change} tasks.md did not exist at the PR base"]
    before = parse_tasks(
        before_text,
        require_test_ids=require_test_ids,
        source=f"{change} base tasks",
    )
    failures: list[str] = []
    if len(before) != len(current_tasks):
        return [f"{change} task metadata changed task count"]
    for old, new in zip(before, current_tasks):
        old_definition = (old.task_id, old.text, old.test_ids, old.section)
        new_definition = (new.task_id, new.text, new.test_ids, new.section)
        if old_definition != new_definition:
            failures.append(f"{change}/{new.task_id} changed task definition in metadata closure")
            continue
        if old.checked == new.checked:
            continue
        if new.task_id not in scoped_task_ids:
            failures.append(f"{change}/{new.task_id} changed checkbox outside PR scope")
        elif old.checked or not new.checked:
            failures.append(f"{change}/{new.task_id} is not an unchecked-to-checked transition")
    return failures


def validate_semantic_metadata(
    root: Path,
    base_sha: str,
    changed_paths: list[str],
    scope: dict[str, set[str]],
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
) -> list[str]:
    """Reject unrelated or substantive changes hidden in metadata-only files."""

    failures: list[str] = []
    changed = set(changed_paths)
    for change, task_ids in scope.items():
        relative = f"openspec/changes/{change}/tasks.md"
        if relative in changed:
            failures.extend(
                validate_task_metadata_transition(
                    change,
                    git_file_at(root, base_sha, relative),
                    task_sets[change],
                    task_ids,
                    mappings[change].contract == "evidence-v2",
                )
            )
    plan_path = "openspec/task-verification.json"
    if plan_path in changed:
        before_text = git_file_at(root, base_sha, plan_path)
        before_rows = plan_rows_from_payload(json.loads(before_text) if before_text else None)
        after_rows = plan_rows_from_payload(
            json.loads((root / plan_path).read_text(encoding="utf-8"))
        )
        for change, task_id in changed_mapping_keys(before_rows, after_rows):
            if task_id not in scope.get(change, set()):
                failures.append(f"verification plan changed outside PR scope: {change}/{task_id}")
    evidence_path = "openspec/task-evidence.json"
    if evidence_path in changed:
        before_text = git_file_at(root, base_sha, evidence_path)
        before_rows = evidence_rows_from_payload(json.loads(before_text) if before_text else None)
        after_rows = evidence_rows_from_payload(
            json.loads((root / evidence_path).read_text(encoding="utf-8"))
        )
        for change, task_id, test_id in changed_mapping_keys(before_rows, after_rows):
            if task_id not in scope.get(change, set()):
                failures.append(
                    f"task evidence changed outside PR scope: {change}/{task_id}/{test_id}"
                )
    return failures


def pull_request_payload(repo: str, number: int) -> dict[str, object]:
    """Fetch the PR fields required for deterministic scope validation."""

    value = gh_json(
        [
            "pr",
            "view",
            str(number),
            "-R",
            repo,
            "--json",
            "body,title,milestone,headRefOid,baseRefOid,headRepository,url,state",
        ]
    )
    return require_object(value, f"pull request #{number}")


def pull_request_files(repo: str, number: int) -> list[str]:
    """Fetch every changed PR path through the paginated REST endpoint."""

    owner, name = repo_parts(repo)
    payload = gh_api_json(
        [
            "--paginate",
            "--slurp",
            "--method",
            "GET",
            f"repos/{owner}/{name}/pulls/{number}/files",
            "-F",
            "per_page=100",
        ]
    )
    return [
        normalized_relative_path(require_object(item, "PR file").get("filename"), "PR filename")
        for item in flatten_paginated_response(payload)
    ]


def check_pr_scope(
    repo: str,
    number: int,
    root: Path,
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
    plan: dict[tuple[str, str], dict[str, object]],
    evidence: dict[tuple[str, str, str], dict[str, object]],
) -> tuple[list[str], set[str]]:
    """Validate linked PR ranges without requiring unrelated milestone completion."""

    pr = pull_request_payload(repo, number)
    body = pr.get("body") or ""
    if not isinstance(body, str):
        raise ValidationError(f"pull request #{number} has a non-text body")
    declared_issues, rows = parse_pr_scope(body)
    scope = expand_pr_scope(declared_issues, rows, mappings, task_sets)
    changed_paths = pull_request_files(repo, number)
    failures = changed_paths_are_owned(changed_paths, scope, plan)
    base_sha = require_string(pr.get("baseRefOid"), "pull request baseRefOid")
    failures.extend(
        validate_semantic_metadata(
            root, base_sha, changed_paths, scope, mappings, task_sets
        )
    )
    for change, task_ids in scope.items():
        if mappings[change].contract != "evidence-v2":
            failures.append(f"PR scope may not add work to legacy contract {change}")
            continue
        failures.extend(checked_evidence_failures(change, task_sets[change], evidence, task_ids))
    return failures, set(scope)


def escape_cell(value: object) -> str:
    """Escape untrusted text for one Markdown table cell."""

    text = html.escape(str(value), quote=True).replace("\r", "").replace("\n", "<br>")
    for character in ("\\", "|", "[", "]", "`", "*", "_", "#"):
        text = text.replace(character, f"\\{character}")
    return text


def marker_for(change: str, section: str) -> str:
    """Return the exact stable marker for one change section."""

    return f"<!-- projectatlas-task-evidence:v1 change={change} section={section} -->"


def derived_evidence_link(row: dict[str, object] | None) -> str:
    """Derive a retained-result link without accepting a caller URL."""

    if row is None:
        return "-"
    retained = require_object(row["retained_result"], "retained_result")
    if retained["kind"] == "github_actions":
        repository = require_string(retained["repository"], "repository")
        run_id = require_positive_int(retained["run_id"], "run_id")
        attempt = require_positive_int(retained["run_attempt"], "run_attempt")
        artifact_id = require_positive_int(retained["artifact_id"], "artifact_id")
        return (
            f"[run {run_id}/{attempt}](https://github.com/{repository}/actions/runs/"
            f"{run_id}/attempts/{attempt}) / "
            f"[artifact {artifact_id}](https://github.com/{repository}/actions/runs/"
            f"{run_id}/artifacts/{artifact_id})"
        )
    path = normalized_relative_path(retained["result_path"], "result_path")
    return f"repository result `{path}`"


def test_source_for(
    plan_row: dict[str, object], test_id: str
) -> dict[str, object] | None:
    """Return the sole validated source declaration for a test identity."""

    matches = [
        require_object(value, "test_source")
        for value in require_array(plan_row.get("test_sources", []), "test_sources")
        if require_object(value, "test_source").get("test_id") == test_id
    ]
    return matches[0] if len(matches) == 1 else None


def derived_test_link(
    root: Path,
    repo: str,
    plan_row: dict[str, object],
    result: dict[str, object] | None,
    test_id: str,
) -> str:
    """Derive an exact tested-commit source permalink from validated metadata."""

    if result is None:
        return escape_cell(test_id)
    source = test_source_for(plan_row, test_id)
    if source is None:
        return escape_cell(test_id)
    tested_commit = require_string(result["tested_commit"], "tested_commit")
    source_path = normalized_relative_path(source["path"], "test_source.path")
    source_anchor = require_string(source["anchor"], "test_source.anchor")
    source_text = git_file_at(root, tested_commit, source_path)
    if source_text is None:
        return escape_cell(test_id)
    line = source_definition_line(source_text, source_path, source_anchor)
    if line is None:
        return escape_cell(test_id)
    encoded_path = quote(source_path, safe="/")
    return (
        f"[{escape_cell(test_id)}](https://github.com/{repo}/blob/"
        f"{tested_commit}/{encoded_path}#L{line})"
    )


def render_section_comment(
    root: Path,
    repo: str,
    change: str,
    section: str,
    tasks: list[Task],
    plan: dict[tuple[str, str], dict[str, object]],
    evidence: dict[tuple[str, str, str], dict[str, object]],
) -> str:
    """Render deterministic non-checkbox evidence rows for one section."""

    lines = [
        marker_for(change, section),
        "",
        "| Task | Unit test | Assertion | Bounded command | Tested commit | Input digest | Run / artifact | Status |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for task in tasks:
        row = plan[(change, task.task_id)]
        command = require_object(row["command"], "command")
        argv = [command["executable"], *require_array(command["arguments"], "arguments")]
        for test_id in task.test_ids:
            result = evidence.get((change, task.task_id, test_id))
            tested_commit = str(result["tested_commit"])[:12] if result else "-"
            digest = str(result["covered_input_digest"])[7:19] if result else "-"
            status = "valid" if task.checked and result else "pending"
            cells = [
                escape_cell(task.task_id),
                derived_test_link(root, repo, row, result, test_id),
                escape_cell(row["assertion"]),
                escape_cell(json.dumps(argv, ensure_ascii=True)),
                escape_cell(tested_commit),
                escape_cell(digest),
                derived_evidence_link(result).replace("|", "\\|"),
                escape_cell(status),
            ]
            lines.append("| " + " | ".join(cells) + " |")
    body = "\n".join(lines) + "\n"
    if len(body) > BODY_CHARACTER_LIMIT:
        raise ValidationError(f"managed evidence comment is too long: {change}/{section}")
    return body


def matching_marker_comments(
    comments: list[dict[str, object]], marker: str
) -> list[dict[str, object]]:
    """Return comments whose first line is exactly the managed marker."""

    matches: list[dict[str, object]] = []
    for comment in comments:
        body = comment.get("body") or ""
        if isinstance(body, str) and normalize_newlines(body).split("\n", 1)[0] == marker:
            matches.append(comment)
    return matches


def owner_for_task(mapping: ChangeMapping, tasks: list[Task], task: Task) -> int:
    """Resolve the sole authoritative issue for one already-validated task."""

    for owner in mapping.owners:
        if task in task_slice(tasks, owner):
            return owner.issue
    raise ValidationError(f"task has no authoritative issue: {task.task_id}")


def build_render_plan(
    repo: str,
    root: Path,
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
    plan: dict[tuple[str, str], dict[str, object]],
    evidence: dict[tuple[str, str, str], dict[str, object]],
    source_digests: dict[str, str],
) -> dict[str, object]:
    """Build idempotent comment operations after authority/evidence validation."""

    operations: list[dict[str, object]] = []
    comment_cache: dict[int, list[dict[str, object]]] = {}
    issue_cache: dict[int, dict[str, object]] = {}
    for change, mapping in sorted(mappings.items()):
        if mapping.contract != "evidence-v2":
            continue
        tasks = task_sets[change]
        sections: dict[str, list[Task]] = {}
        for task in tasks:
            sections.setdefault(task.section, []).append(task)
        for section, section_tasks in sections.items():
            issues = {owner_for_task(mapping, tasks, task) for task in section_tasks}
            if len(issues) != 1:
                raise ValidationError(f"{change} section {section} crosses authority issues")
            issue_number = next(iter(issues))
            comments = comment_cache.setdefault(issue_number, issue_comments(repo, issue_number))
            issue = issue_cache.setdefault(issue_number, issue_payload(repo, issue_number))
            marker = marker_for(change, section)
            matches = matching_marker_comments(comments, marker)
            if len(matches) > 1:
                raise ValidationError(f"#{issue_number} has duplicate managed marker {marker}")
            body = render_section_comment(
                root, repo, change, section, section_tasks, plan, evidence
            )
            existing = matches[0] if matches else None
            existing_body = existing.get("body") if existing else None
            action = "create" if existing is None else ("noop" if existing_body == body else "update")
            operations.append(
                {
                    "action": action,
                    "issue": issue_number,
                    "marker": marker,
                    "comment_id": existing.get("id") if existing else None,
                    "expected_comment_digest": sha256_digest(
                        str(existing_body or "").encode("utf-8")
                    ),
                    "expected_comment_updated_at": existing.get("updated_at") if existing else None,
                    "expected_issue_updated_at": issue.get("updated_at"),
                    "expected_issue_body_digest": sha256_digest(
                        str(issue.get("body") or "").encode("utf-8")
                    ),
                    "body": body,
                }
            )
    return {
        "schema_version": RENDER_SCHEMA_VERSION,
        "repository": repo,
        "source_digests": source_digests,
        "operations": operations,
    }


def load_provenance(path: Path, repo: str) -> dict[str, object]:
    """Load strict writeback provenance without accepting URLs or status aliases."""

    value = require_object(json.loads(path.read_text(encoding="utf-8")), str(path))
    required = {
        "schema_version",
        "repository",
        "workflow_name",
        "event",
        "conclusion",
        "run_id",
        "run_attempt",
        "job_id",
        "job_name",
        "artifact_id",
        "artifact_name",
        "artifact_digest",
        "head_repository",
        "head_sha",
        "pull_request",
    }
    require_exact_keys(value, required, str(path))
    if value["schema_version"] != PROVENANCE_SCHEMA_VERSION:
        raise ValidationError("unsupported provenance schema")
    if value["repository"] != repo or value["head_repository"] != repo:
        raise ValidationError("fork or foreign repository writeback is refused")
    if value["workflow_name"] != "01-CI" or value["event"] != "pull_request":
        raise ValidationError("writeback provenance names the wrong workflow or event")
    if value["conclusion"] != "success":
        raise ValidationError("writeback provenance is not successful")
    for key in ("run_id", "run_attempt", "job_id", "artifact_id", "pull_request"):
        require_positive_int(value[key], key)
    for key in ("job_name", "artifact_name"):
        require_string(value[key], key)
    if not SHA_RE.fullmatch(require_string(value["head_sha"], "head_sha")):
        raise ValidationError("head_sha is not canonical")
    if not DIGEST_RE.fullmatch(require_string(value["artifact_digest"], "artifact_digest")):
        raise ValidationError("artifact_digest is not canonical")
    return value


def paginated_object_items(payload: object, key: str) -> list[dict[str, object]]:
    """Flatten one `gh api --paginate --slurp` object-array field."""

    pages = require_array(payload, f"paginated {key} response")
    items: list[dict[str, object]] = []
    for position, value in enumerate(pages):
        page = require_object(value, f"paginated {key} page[{position}]")
        for item in require_array(page.get(key), f"paginated {key} page[{position}].{key}"):
            items.append(require_object(item, f"{key} item"))
    return items


def verify_actions_identity(
    repo: str,
    *,
    run_id: int,
    attempt: int,
    job_id: int,
    job_name: str,
    artifact_id: int,
    artifact_name: str,
    artifact_digest: str,
    head_sha: str,
    workflow_name: str | None = None,
    event: str | None = None,
    pull_request: int | None = None,
    cache: dict[str, dict[object, object]] | None = None,
) -> None:
    """Reconcile one Actions result against fresh run, job, and artifact APIs."""

    owner, name = repo_parts(repo)
    cache = cache if cache is not None else {}
    runs = cache.setdefault("runs", {})
    jobs_by_attempt = cache.setdefault("jobs", {})
    artifacts_by_run = cache.setdefault("artifacts", {})
    if run_id not in runs:
        runs[run_id] = require_object(
            gh_api_json([f"repos/{owner}/{name}/actions/runs/{run_id}"]),
            "workflow run",
        )
    run_value = require_object(runs[run_id], "workflow run")
    expected_run: dict[str, object] = {
        "id": run_id,
        "run_attempt": attempt,
        "status": "completed",
        "conclusion": "success",
        "head_sha": head_sha,
    }
    if workflow_name is not None:
        expected_run["name"] = workflow_name
    if event is not None:
        expected_run["event"] = event
    for key, expected in expected_run.items():
        if run_value.get(key) != expected:
            raise ValidationError(f"workflow run provenance mismatch for {key}")
    for key in ("head_repository", "repository"):
        repository = require_object(run_value.get(key), f"workflow {key}")
        if repository.get("full_name") != repo:
            raise ValidationError(f"workflow run has a foreign {key}")
    if pull_request is not None:
        pulls = require_array(run_value.get("pull_requests"), "workflow run pull_requests")
        if (
            len(pulls) != 1
            or require_object(pulls[0], "workflow PR").get("number") != pull_request
        ):
            raise ValidationError("workflow run does not identify exactly one expected PR")

    job_key = (run_id, attempt)
    if job_key not in jobs_by_attempt:
        jobs_by_attempt[job_key] = paginated_object_items(
            gh_api_json(
                [
                    "--paginate",
                    "--slurp",
                    "--method",
                    "GET",
                    f"repos/{owner}/{name}/actions/runs/{run_id}/attempts/{attempt}/jobs",
                    "-F",
                    "per_page=100",
                ]
            ),
            "jobs",
        )
    jobs = require_array(jobs_by_attempt[job_key], "cached jobs")
    matching_jobs = [
        require_object(job, "job")
        for job in jobs
        if require_object(job, "job").get("id") == job_id
    ]
    if len(matching_jobs) != 1:
        raise ValidationError("expected producer job was not found")
    job = matching_jobs[0]
    if (
        job.get("name") != job_name
        or job.get("conclusion") != "success"
        or job.get("run_attempt") != attempt
        or job.get("head_sha") != head_sha
    ):
        raise ValidationError("producer job identity, conclusion, attempt, or SHA differs")

    if run_id not in artifacts_by_run:
        artifacts_by_run[run_id] = paginated_object_items(
            gh_api_json(
                [
                    "--paginate",
                    "--slurp",
                    "--method",
                    "GET",
                    f"repos/{owner}/{name}/actions/runs/{run_id}/artifacts",
                    "-F",
                    "per_page=100",
                ]
            ),
            "artifacts",
        )
    artifacts = require_array(artifacts_by_run[run_id], "cached artifacts")
    matching_artifacts = [
        require_object(artifact, "artifact")
        for artifact in artifacts
        if require_object(artifact, "artifact").get("id") == artifact_id
    ]
    if len(matching_artifacts) != 1:
        raise ValidationError("expected producer artifact was not found")
    artifact = matching_artifacts[0]
    artifact_run = require_object(artifact.get("workflow_run"), "artifact workflow_run")
    if (
        artifact.get("name") != artifact_name
        or artifact.get("digest") != artifact_digest
        or artifact.get("expired") is not False
        or require_positive_int(artifact.get("size_in_bytes"), "artifact size")
        > MAX_ARTIFACT_BYTES
        or artifact_run.get("id") != run_id
        or artifact_run.get("head_sha") != head_sha
    ):
        raise ValidationError("producer artifact identity, digest, expiry, size, or SHA differs")


def verify_live_provenance(
    repo: str,
    provenance: dict[str, object],
    cache: dict[str, dict[object, object]] | None = None,
) -> None:
    """Reconcile renderer provenance and the still-current same-repo PR head."""

    verify_actions_identity(
        repo,
        run_id=require_positive_int(provenance["run_id"], "run_id"),
        attempt=require_positive_int(provenance["run_attempt"], "run_attempt"),
        job_id=require_positive_int(provenance["job_id"], "job_id"),
        job_name=require_string(provenance["job_name"], "job_name"),
        artifact_id=require_positive_int(provenance["artifact_id"], "artifact_id"),
        artifact_name=require_string(provenance["artifact_name"], "artifact_name"),
        artifact_digest=require_string(provenance["artifact_digest"], "artifact_digest"),
        head_sha=require_string(provenance["head_sha"], "head_sha"),
        workflow_name=require_string(provenance["workflow_name"], "workflow_name"),
        event="pull_request",
        pull_request=require_positive_int(provenance["pull_request"], "pull_request"),
        cache=cache,
    )
    pr = pull_request_payload(repo, int(provenance["pull_request"]))
    head_repo = require_object(pr.get("headRepository"), "PR headRepository")
    if (
        pr.get("headRefOid") != provenance["head_sha"]
        or head_repo.get("nameWithOwner") != repo
        or str(pr.get("state", "")).upper() != "OPEN"
    ):
        raise ValidationError("PR head moved, closed, or belongs to a foreign repository")


def verify_live_evidence_results(
    repo: str,
    evidence: dict[tuple[str, str, str], dict[str, object]],
    cache: dict[str, dict[object, object]] | None = None,
) -> None:
    """Reconcile every Actions-backed task result against its tested commit."""

    cache = cache if cache is not None else {}
    for identity, row in sorted(evidence.items()):
        retained = require_object(row["retained_result"], "retained_result")
        if retained["kind"] != "github_actions":
            continue
        retained_repo = require_string(retained["repository"], "retained_result.repository")
        if retained_repo != repo:
            raise ValidationError(f"{identity} evidence belongs to a foreign repository")
        verify_actions_identity(
            repo,
            run_id=require_positive_int(retained["run_id"], "retained_result.run_id"),
            attempt=require_positive_int(
                retained["run_attempt"], "retained_result.run_attempt"
            ),
            job_id=require_positive_int(retained["job_id"], "retained_result.job_id"),
            job_name=require_string(retained["job_name"], "retained_result.job_name"),
            artifact_id=require_positive_int(
                retained["artifact_id"], "retained_result.artifact_id"
            ),
            artifact_name=require_string(
                retained["artifact_name"], "retained_result.artifact_name"
            ),
            artifact_digest=require_string(
                retained["artifact_digest"], "retained_result.artifact_digest"
            ),
            head_sha=require_string(row["tested_commit"], "tested_commit"),
            cache=cache,
        )


def validate_render_plan(value: object, repo: str) -> dict[str, object]:
    """Validate a render plan before comparing it with a fresh recomputation."""

    plan = require_object(value, "render plan")
    require_exact_keys(plan, {"schema_version", "repository", "source_digests", "operations"}, "render plan")
    if plan["schema_version"] != RENDER_SCHEMA_VERSION or plan["repository"] != repo:
        raise ValidationError("render plan schema or repository differs")
    require_object(plan["source_digests"], "source_digests")
    for position, value in enumerate(require_array(plan["operations"], "operations")):
        operation = require_object(value, f"operations[{position}]")
        require_exact_keys(
            operation,
            {
                "action",
                "issue",
                "marker",
                "comment_id",
                "expected_comment_digest",
                "expected_comment_updated_at",
                "expected_issue_updated_at",
                "expected_issue_body_digest",
                "body",
            },
            f"operations[{position}]",
        )
        if operation["action"] not in {"create", "update", "noop"}:
            raise ValidationError("render plan has an unsupported action")
        require_positive_int(operation["issue"], "operation.issue")
        marker = require_string(operation["marker"], "operation.marker")
        if not MARKER_RE.fullmatch(marker):
            raise ValidationError("render plan has a malformed marker")
        if not DIGEST_RE.fullmatch(require_string(operation["expected_comment_digest"], "comment digest")):
            raise ValidationError("render plan has a malformed comment digest")
        if not DIGEST_RE.fullmatch(require_string(operation["expected_issue_body_digest"], "body digest")):
            raise ValidationError("render plan has a malformed issue digest")
        body = require_string(operation["body"], "operation.body")
        if normalize_newlines(body).split("\n", 1)[0] != marker:
            raise ValidationError("render body marker differs from its operation")
    return plan


def apply_render_plan(
    repo: str,
    plan: dict[str, object],
    provenance: dict[str, object],
    evidence: dict[tuple[str, str, str], dict[str, object]],
) -> None:
    """Apply a fresh plan after rechecking provenance, issues, and comments."""

    owner, name = repo_parts(repo)
    actions_cache: dict[str, dict[object, object]] = {}
    verify_live_provenance(repo, provenance, actions_cache)
    verify_live_evidence_results(repo, evidence, actions_cache)
    for value in require_array(plan["operations"], "operations"):
        operation = require_object(value, "operation")
        if operation["action"] == "noop":
            continue
        issue_number = int(operation["issue"])
        issue = issue_payload(repo, issue_number)
        if (
            issue.get("updated_at") != operation["expected_issue_updated_at"]
            or sha256_digest(str(issue.get("body") or "").encode("utf-8"))
            != operation["expected_issue_body_digest"]
        ):
            raise ValidationError(f"#{issue_number} changed after render planning")
        marker = str(operation["marker"])
        matches = matching_marker_comments(issue_comments(repo, issue_number), marker)
        if operation["action"] == "create":
            if matches:
                raise ValidationError(f"#{issue_number} marker appeared concurrently")
            gh_api_write(
                f"repos/{owner}/{name}/issues/{issue_number}/comments",
                "POST",
                {"body": operation["body"]},
            )
        else:
            if len(matches) != 1 or matches[0].get("id") != operation["comment_id"]:
                raise ValidationError(f"#{issue_number} managed comment changed concurrently")
            existing_body = str(matches[0].get("body") or "")
            if (
                matches[0].get("updated_at") != operation["expected_comment_updated_at"]
                or sha256_digest(existing_body.encode("utf-8"))
                != operation["expected_comment_digest"]
            ):
                raise ValidationError(f"#{issue_number} managed comment changed concurrently")
            gh_api_write(
                f"repos/{owner}/{name}/issues/comments/{operation['comment_id']}",
                "PATCH",
                {"body": operation["body"]},
            )


def run_rust_task_validator(
    root: Path,
    policy: str,
    tasks: str,
    plan: str,
    evidence: str,
    expected_commit: str,
) -> None:
    """Invoke the typed Rust validator through its fixed argument contract."""

    for value, name in ((policy, "policy"), (tasks, "tasks"), (plan, "plan"), (evidence, "evidence")):
        normalized_relative_path(value, name)
    if not SHA_RE.fullmatch(expected_commit):
        raise ValidationError("expected commit is not canonical")
    run(
        [
            "cargo",
            "projectatlas-lints",
            "test-quality",
            "tasks",
            "--root",
            str(root),
            "--policy",
            policy,
            "--tasks",
            tasks,
            "--plan",
            plan,
            "--evidence",
            evidence,
            "--expected-commit",
            expected_commit,
            "--json",
        ]
    )


def expect_failure(action, contains: str) -> None:
    """Require one self-test action to fail with an expected diagnostic fragment."""

    try:
        action()
    except ValidationError as error:
        if contains not in str(error):
            raise AssertionError(f"expected {contains!r} in {error!r}") from error
    else:
        raise AssertionError(f"expected ValidationError containing {contains!r}")


def self_test(root: Path, only_test_id: str | None = None) -> None:
    """Run hostile IssueOps fixtures for `TQG-UT-2.1` through `2.8`."""

    fixture_path = root / ".github" / "fixtures" / "issueops" / "cases.json"
    fixture = require_object(
        json.loads(fixture_path.read_text(encoding="utf-8")), str(fixture_path)
    )
    require_exact_keys(
        fixture,
        {
            "schema_version",
            "cases",
            "markdown_cells",
            "paginated",
            "duplicate_comments",
        },
        str(fixture_path),
    )
    if fixture["schema_version"] != 2:
        raise AssertionError("fixture schema version differs")
    module = sys.modules[__name__]

    def clone_json(value: object) -> object:
        return json.loads(json.dumps(value))

    def write_json(path: Path, value: object) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(value, indent=2, ensure_ascii=True) + "\n", encoding="utf-8"
        )

    def git(directory: Path, *arguments: str) -> str:
        process = subprocess.run(
            ["git", "-C", str(directory), *arguments],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        if process.returncode:
            raise AssertionError(
                f"self-test git command failed: {arguments}: {process.stderr.strip()}"
            )
        return normalize_newlines(process.stdout).strip()

    def commit_all(directory: Path, message: str) -> str:
        git(directory, "add", ".")
        git(directory, "commit", "-m", message)
        commit = git(directory, "rev-parse", "HEAD")
        assert SHA_RE.fullmatch(commit)
        return commit

    def simple_task(
        task_id: str = "1.1",
        *,
        checked: bool = False,
        text: str = "Do work",
        test_id: str | None = "TQG-UT-1.1",
        section: str = "1",
    ) -> Task:
        suffix = f" `{test_id}`" if test_id else ""
        return Task(
            checked,
            task_id,
            f"{task_id} {text}{suffix}",
            (test_id,) if test_id else (),
            section,
        )

    def simple_plan_row(
        task: Task, covered_paths: list[str] | None = None
    ) -> dict[str, object]:
        return {
            "task_id": task.task_id,
            "test_ids": list(task.test_ids),
            "assertion": "Verify do work",
            "command": {
                "executable": "cargo",
                "arguments": [
                    "test",
                    "--locked",
                    "-p",
                    "projectatlas-lints",
                    "task_fixture",
                ],
            },
            "timeout_seconds": 120,
            "covered_inputs": [
                {"kind": "file", "path": value}
                for value in (
                    covered_paths
                    or [
                        "openspec/changes/change/tasks.md",
                        "openspec/task-verification.json",
                        "openspec/task-evidence.json",
                        "src/lib.rs",
                    ]
                )
            ],
        }

    def create_evidence_fixture(directory: str) -> dict[str, object]:
        fixture_root = Path(directory)
        task_path = fixture_root / "openspec/changes/change/tasks.md"
        plan_path = fixture_root / "openspec/task-verification.json"
        evidence_path = fixture_root / "openspec/task-evidence.json"
        source_path = fixture_root / "src/lib.rs"
        result_path = fixture_root / "artifacts/task-result.txt"
        for path in (task_path, source_path, result_path):
            path.parent.mkdir(parents=True, exist_ok=True)
        task_path.write_text(
            "## 1. Test\n\n"
            "- [ ] 1.1 Do work; unit test `TQG-UT-1.1` passes.\n",
            encoding="utf-8",
        )
        source_path.write_text("pub fn value() -> u8 { 1 }\n", encoding="utf-8")
        result_path.write_text("task passed\n", encoding="utf-8")
        initial_task = parse_tasks(
            task_path.read_text(encoding="utf-8"),
            require_test_ids=True,
            source="fixture tasks",
        )[0]
        plan_row = simple_plan_row(initial_task)
        plan_payload = {
            "schema_version": 1,
            "changes": {"change": {"tasks": [plan_row]}},
        }
        write_json(plan_path, plan_payload)
        write_json(evidence_path, {"schema_version": 1, "results": []})
        git(fixture_root, "init")
        git(fixture_root, "config", "user.email", "issueops@example.invalid")
        git(fixture_root, "config", "user.name", "IssueOps Self Test")
        tested_commit = commit_all(fixture_root, "tested implementation")

        task_path.write_text(
            "## 1. Test\n\n"
            "- [x] 1.1 Do work; unit test `TQG-UT-1.1` passes.\n",
            encoding="utf-8",
        )
        current_task = parse_tasks(
            task_path.read_text(encoding="utf-8"),
            require_test_ids=True,
            source="fixture tasks",
        )[0]
        mappings = {
            "change": ChangeMapping(
                "evidence-v2", 1, (Owner(1, "1.1", "1.1"),)
            )
        }
        task_sets = {"change": [current_task]}
        plan_index = {("change", "1.1"): plan_row}
        evidence_row = {
            "change": "change",
            "task_id": "1.1",
            "test_id": "TQG-UT-1.1",
            "outcome": "passed",
            "tested_commit": tested_commit,
            "covered_input_digest": covered_input_digest(fixture_root, plan_row),
            "platform": {"os": "linux", "arch": "x86_64"},
            "started_at": "2026-07-11T10:00:00Z",
            "completed_at": "2026-07-11T10:00:01Z",
            "retained_result": {
                "kind": "repository",
                "result_path": "artifacts/task-result.txt",
                "result_digest": sha256_digest(result_path.read_bytes()),
            },
        }
        evidence_payload = {"schema_version": 1, "results": [evidence_row]}
        write_json(evidence_path, evidence_payload)
        closure_commit = commit_all(fixture_root, "metadata closure")
        return {
            "root": fixture_root,
            "task_path": task_path,
            "plan_path": plan_path,
            "evidence_path": evidence_path,
            "source_path": source_path,
            "result_path": result_path,
            "tested_commit": tested_commit,
            "closure_commit": closure_commit,
            "mappings": mappings,
            "task_sets": task_sets,
            "plan_index": plan_index,
            "plan_row": plan_row,
            "evidence_row": evidence_row,
            "evidence_payload": evidence_payload,
        }

    def load_fixture_evidence(
        values: dict[str, object], payload: object | None = None
    ) -> tuple[dict[tuple[str, str, str], dict[str, object]], dict[str, object]]:
        evidence_path = values["evidence_path"]
        assert isinstance(evidence_path, Path)
        selected = payload if payload is not None else values["evidence_payload"]
        write_json(evidence_path, selected)
        return load_evidence(
            evidence_path,
            values["root"],
            values["mappings"],
            values["task_sets"],
            values["plan_index"],
        )

    def valid_retained_actions(
        *, repository: str = "owner/repo", sha_digest: str | None = None
    ) -> dict[str, object]:
        digest = sha_digest or ("sha256:" + "a" * 64)
        return {
            "kind": "github_actions",
            "repository": repository,
            "run_id": 1,
            "run_attempt": 1,
            "job_id": 2,
            "job_name": "verify",
            "artifact_id": 3,
            "artifact_name": "projectatlas-issueops-v1",
            "artifact_digest": digest,
            "result_path": "results/task.json",
            "result_digest": "sha256:" + "b" * 64,
        }

    def valid_provenance(head_sha: str = "a" * 40) -> dict[str, object]:
        return {
            "schema_version": 1,
            "repository": "owner/repo",
            "workflow_name": "01-CI",
            "event": "pull_request",
            "conclusion": "success",
            "run_id": 1,
            "run_attempt": 1,
            "job_id": 2,
            "job_name": "verify",
            "artifact_id": 3,
            "artifact_name": "projectatlas-issueops-v1",
            "artifact_digest": "sha256:" + "a" * 64,
            "head_repository": "owner/repo",
            "head_sha": head_sha,
            "pull_request": 4,
        }

    def live_api(
        head_sha: str = "a" * 40,
        *,
        calls: list[list[str]] | None = None,
        run_updates: dict[str, object] | None = None,
        job_updates: dict[str, object] | None = None,
        artifact_updates: dict[str, object] | None = None,
    ):
        run = {
            "id": 1,
            "run_attempt": 1,
            "name": "01-CI",
            "event": "pull_request",
            "status": "completed",
            "conclusion": "success",
            "head_sha": head_sha,
            "head_repository": {"full_name": "owner/repo"},
            "repository": {"full_name": "owner/repo"},
            "pull_requests": [{"number": 4}],
        }
        job = {
            "id": 2,
            "name": "verify",
            "conclusion": "success",
            "run_attempt": 1,
            "head_sha": head_sha,
        }
        artifact = {
            "id": 3,
            "name": "projectatlas-issueops-v1",
            "digest": "sha256:" + "a" * 64,
            "expired": False,
            "size_in_bytes": 100,
            "workflow_run": {"id": 1, "head_sha": head_sha},
        }
        run.update(run_updates or {})
        job.update(job_updates or {})
        artifact.update(artifact_updates or {})

        def api(arguments: list[str]) -> object:
            if calls is not None:
                calls.append(list(arguments))
            endpoint = next(
                value for value in arguments if value.startswith("repos/")
            )
            if endpoint.endswith("/jobs"):
                return [{"jobs": [job]}]
            if endpoint.endswith("/artifacts"):
                return [{"artifacts": [artifact]}]
            return run

        return api

    def valid_pr(head_sha: str = "a" * 40) -> dict[str, object]:
        return {
            "headRefOid": head_sha,
            "headRepository": {"nameWithOwner": "owner/repo"},
            "state": "OPEN",
        }

    def renderer_context() -> dict[str, object]:
        task = simple_task(checked=False)
        row = simple_plan_row(task, ["src/lib.rs"])
        mapping = ChangeMapping(
            "evidence-v2", 1, (Owner(1, "1.1", "1.1"),)
        )
        task_sets = {"change": [task]}
        plan = {("change", "1.1"): row}
        issue = {
            "body": "Issue body",
            "updated_at": "2026-07-11T10:00:00Z",
        }
        desired = render_section_comment(
            root, "owner/repo", "change", "1", [task], plan, {}
        )
        return {
            "task": task,
            "mappings": {"change": mapping},
            "task_sets": task_sets,
            "plan": plan,
            "issue": issue,
            "desired": desired,
        }

    def build_fixture_render_plan(
        context: dict[str, object], comments: list[dict[str, object]]
    ) -> dict[str, object]:
        with (
            patch.object(module, "issue_comments", return_value=comments),
            patch.object(module, "issue_payload", return_value=context["issue"]),
        ):
            return build_render_plan(
                "owner/repo",
                root,
                context["mappings"],
                context["task_sets"],
                context["plan"],
                {},
                {"fixture": "sha256:" + "c" * 64},
            )

    def test_2_1() -> None:
        actual = parse_tasks(
            (root / TQG_TASK_PATH).read_text(encoding="utf-8"),
            require_test_ids=True,
            source=TQG_TASK_PATH,
        )
        assert actual
        assert all(task.test_ids for task in actual)
        valid = "## 1. Test\n\n- [ ] 1.1 Do work; unit test `TQG-UT-1.1` passes.\n"
        assert parse_tasks(
            valid, require_test_ids=True, source="fixture"
        )[0].test_ids == ("TQG-UT-1.1",)
        tagged = "## 1. Test\n\n- [ ] 1.1 Do work. [UT:ARRI-1.1]\n"
        assert parse_tasks(
            tagged, require_test_ids=True, source="fixture"
        )[0].test_ids == ("UT:ARRI-1.1",)
        expect_failure(
            lambda: parse_tasks(
                "## 1. Test\n- [ ] 1.1 Missing\n",
                require_test_ids=True,
                source="fixture",
            ),
            "no test identifier",
        )
        expect_failure(
            lambda: parse_tasks(
                valid + "- [ ] 1.1 Duplicate `TQG-UT-1.2`\n",
                require_test_ids=True,
                source="fixture",
            ),
            "duplicates task",
        )
        expect_failure(
            lambda: parse_tasks(
                "## 1. Test\n- [ ] 1..1 Malformed `TQG-UT-1.1`\n",
                require_test_ids=True,
                source="fixture",
            ),
            "non-canonical task row",
        )
        expect_failure(
            lambda: parse_tasks(
                "## 1. Test\n- [ ] 1.1 Malformed `tqg-ut-1.1`\n",
                require_test_ids=True,
                source="fixture",
            ),
            "no test identifier",
        )
        expect_failure(
            lambda: parse_tasks(
                "## 1. Test\n"
                "- [ ] 1.1 Repeated `TQG-UT-1.1` and `TQG-UT-1.1`\n",
                require_test_ids=True,
                source="fixture",
            ),
            "repeats a test identifier",
        )
        with tempfile.TemporaryDirectory() as directory:
            fixture_root = Path(directory)
            change_dir = fixture_root / "openspec/changes/change"
            change_dir.mkdir(parents=True)
            (change_dir / "tasks.md").write_text(
                "## 1. Test\n"
                "- [ ] 1.1 First `TQG-UT-1.1`\n"
                "- [ ] 1.2 Second `TQG-UT-1.1`\n",
                encoding="utf-8",
            )
            issue_map = {
                "schema_version": 2,
                "changes": {
                    "change": {
                        "contract": "evidence-v2",
                        "primary_issue": 1,
                        "owners": [
                            {
                                "issue": 1,
                                "first_task": "1.1",
                                "last_task": "1.2",
                            }
                        ],
                    }
                },
            }
            map_path = fixture_root / "openspec/issue-map.json"
            write_json(map_path, issue_map)
            expect_failure(
                lambda: load_issue_map(map_path, fixture_root),
                "is reused",
            )

    def test_2_2() -> None:
        mappings, task_sets = load_issue_map(
            root / "openspec/issue-map.json", root
        )
        plan, payload = load_verification_plan(
            root / "openspec/task-verification.json", mappings, task_sets
        )
        assert len(plan) == sum(
            len(task_sets[change])
            for change, mapping in mappings.items()
            if mapping.contract == "evidence-v2"
        )
        assert canonical_json(json.loads(json.dumps(payload))) == canonical_json(
            payload
        )
        command = {"executable": "cargo", "arguments": ["test", "--locked"]}
        assert validate_command(json.loads(json.dumps(command)), "command") == command
        expect_failure(
            lambda: validate_command(
                {"executable": "cargo test", "arguments": []}, "command"
            ),
            "shell command string",
        )
        actions = valid_retained_actions()
        assert (
            validate_retained_result(
                json.loads(json.dumps(actions)), "retained_result"
            )
            == actions
        )
        for field in ("url", "status"):
            hostile = clone_json(actions)
            assert isinstance(hostile, dict)
            hostile[field] = "caller-controlled"
            expect_failure(
                lambda hostile=hostile: validate_retained_result(
                    hostile, "retained_result"
                ),
                "unknown fields",
            )
        missing = clone_json(actions)
        assert isinstance(missing, dict)
        del missing["job_id"]
        expect_failure(
            lambda: validate_retained_result(missing, "retained_result"),
            "missing fields",
        )
        zero_identity = clone_json(actions)
        assert isinstance(zero_identity, dict)
        zero_identity["run_attempt"] = 0
        expect_failure(
            lambda: validate_retained_result(
                zero_identity, "retained_result"
            ),
            "positive integer",
        )
        with tempfile.TemporaryDirectory() as directory:
            changed = clone_json(payload)
            assert isinstance(changed, dict)
            changed["changes"][TQG_CHANGE]["tasks"][0]["timeout_seconds"] = 0
            path = Path(directory) / "plan.json"
            write_json(path, changed)
            expect_failure(
                lambda: load_verification_plan(path, mappings, task_sets),
                "positive integer",
            )
            changed = clone_json(payload)
            assert isinstance(changed, dict)
            changed["changes"][TQG_CHANGE]["tasks"][0][
                "assertion"
            ] = "caller\ncontrol"
            write_json(path, changed)
            expect_failure(
                lambda: load_verification_plan(path, mappings, task_sets),
                "bounded plain text",
            )
        required = TQG_REQUIRED_SECTION_INPUTS["2"]
        assert {
            ".github/scripts/issue-checklists.py",
            "openspec/task-verification.json",
            "openspec/task-evidence.json",
        }.issubset(required)

    def test_2_3() -> None:
        with tempfile.TemporaryDirectory() as directory:
            values = create_evidence_fixture(directory)
            unrelated = Path(directory) / "docs/unrelated.md"
            unrelated.parent.mkdir(parents=True)
            unrelated.write_text("later unrelated work\n", encoding="utf-8")
            commit_all(Path(directory), "unrelated later work")
            results, payload = load_fixture_evidence(values)
            assert len(results) == 1
            assert payload == values["evidence_payload"]

            failed = clone_json(values["evidence_payload"])
            failed["results"][0]["outcome"] = "failed"
            expect_failure(
                lambda: load_fixture_evidence(values, failed),
                "must be passing",
            )
            stale = clone_json(values["evidence_payload"])
            stale["results"][0]["covered_input_digest"] = "sha256:" + "0" * 64
            expect_failure(
                lambda: load_fixture_evidence(values, stale),
                "stale for its covered inputs",
            )
            wrong_commit = clone_json(values["evidence_payload"])
            wrong_commit["results"][0]["tested_commit"] = "f" * 40
            expect_failure(
                lambda: load_fixture_evidence(values, wrong_commit),
                "not an ancestor",
            )
            wrong_test = clone_json(values["evidence_payload"])
            wrong_test["results"][0]["test_id"] = "TQG-UT-9.9"
            expect_failure(
                lambda: load_fixture_evidence(values, wrong_test),
                "wrong test",
            )
            duplicate = clone_json(values["evidence_payload"])
            duplicate["results"].append(clone_json(duplicate["results"][0]))
            expect_failure(
                lambda: load_fixture_evidence(values, duplicate),
                "duplicates change/1.1/TQG-UT-1.1",
            )
            missing_retained = clone_json(values["evidence_payload"])
            del missing_retained["results"][0]["retained_result"]
            expect_failure(
                lambda: load_fixture_evidence(values, missing_retained),
                "missing fields",
            )
            result_path = values["result_path"]
            assert isinstance(result_path, Path)
            result_bytes = result_path.read_bytes()
            result_path.unlink()
            expect_failure(
                lambda: load_fixture_evidence(values),
                "repository result is missing",
            )
            result_path.write_bytes(result_bytes)

            plan_row = clone_json(values["plan_row"])
            assert isinstance(plan_row, dict)
            plan_row["assertion"] = "Verify a different assertion"
            expect_failure(
                lambda: validate_tested_commit(
                    values["root"],
                    "change",
                    "1.1",
                    values["tested_commit"],
                    plan_row,
                ),
                "plan assertion differs",
            )

            source_path = values["source_path"]
            assert isinstance(source_path, Path)
            source_path.write_text(
                "pub fn value() -> u8 { 2 }\n", encoding="utf-8"
            )
            changed_input = clone_json(values["evidence_payload"])
            changed_input["results"][0][
                "covered_input_digest"
            ] = covered_input_digest(values["root"], values["plan_row"])
            write_json(values["evidence_path"], changed_input)
            commit_all(Path(directory), "invalid closure changes covered input")
            expect_failure(
                lambda: load_fixture_evidence(values, changed_input),
                "covered input differs from the tested commit",
            )

    def test_2_4() -> None:
        source_task = Task(
            True,
            "1.1",
            "1.1 Implement source `TQG-UT-1.1`",
            ("TQG-UT-1.1",),
            "1",
        )
        docs_task = Task(
            True,
            "1.2",
            "1.2 Update docs `TQG-UT-1.2`",
            ("TQG-UT-1.2",),
            "1",
        )
        failures = checked_evidence_failures(
            "change", [source_task, docs_task], {}
        )
        assert len(failures) == 2
        assert all("lacks current evidence" in value for value in failures)
        evidence = {
            ("change", "1.1", "TQG-UT-1.1"): {"outcome": "passed"},
            ("change", "1.2", "TQG-UT-1.2"): {"outcome": "passed"},
        }
        assert (
            checked_evidence_failures(
                "change", [source_task, docs_task], evidence
            )
            == []
        )
        pending = Task(
            False,
            source_task.task_id,
            source_task.text,
            source_task.test_ids,
            source_task.section,
        )
        failures = checked_evidence_failures(
            "change", [pending], evidence, {"1.1"}
        )
        assert failures == ["change/1.1 is in PR scope but is not complete"]

    def test_2_5() -> None:
        first = simple_task("1.1", text="First", test_id="TQG-UT-1.1")
        second = simple_task("1.2", text="Second", test_id="TQG-UT-1.2")
        third = simple_task("1.3", text="Third", test_id="TQG-UT-1.3")
        assert compare_task_sequences(
            [first, second], [first, second], "fixture"
        ) == []
        assert compare_task_sequences(
            [first, second], [first], "missing"
        )
        assert compare_task_sequences(
            [first, second], [first, second, third], "extra"
        )
        assert compare_task_sequences(
            [first, second], [second, first], "reordered"
        )
        checked_first = Task(
            True, first.task_id, first.text, first.test_ids, first.section
        )
        assert compare_task_sequences(
            [first, second], [checked_first, second], "state drift"
        )
        stale_text = simple_task(
            "1.1", text="Changed", test_id="TQG-UT-1.1"
        )
        assert compare_task_sequences(
            [first, second], [stale_text, second], "text drift"
        )
        expect_failure(
            lambda: parse_tasks(
                "## 1. Test\n"
                "- [ ] 1.1 First `TQG-UT-1.1`\n"
                "- [ ] 1.1 Duplicate `TQG-UT-1.2`\n",
                require_test_ids=True,
                source="duplicate",
            ),
            "duplicates task",
        )
        pages = fixture["paginated"]
        assert flatten_paginated_response(pages) == [
            {"number": 1},
            {"number": 2},
        ]
        expect_failure(
            lambda: flatten_paginated_response(
                [[{"number": 1}], {"number": 2}]
            ),
            "mixes pages and items",
        )
        for value in require_array(fixture["markdown_cells"], "markdown_cells"):
            escaped = escape_cell(value)
            assert "<script>" not in escaped
            assert "\n" not in escaped
            assert "|" not in escaped.replace("\\|", "")
        marker = marker_for("change", "1")
        duplicate_comments = [
            require_object(value, "duplicate comment")
            for value in require_array(
                fixture["duplicate_comments"], "duplicate_comments"
            )
        ]
        assert len(matching_marker_comments(duplicate_comments, marker)) == 2

        context = renderer_context()
        existing = {
            "id": 10,
            "body": context["desired"],
            "updated_at": "2026-07-11T10:00:01Z",
        }
        first_plan = build_fixture_render_plan(context, [existing])
        second_plan = build_fixture_render_plan(context, [existing])
        assert canonical_json(first_plan) == canonical_json(second_plan)
        assert first_plan["operations"][0]["action"] == "noop"

        old_comment = {
            "id": 10,
            "body": marker + "\nold",
            "updated_at": "2026-07-11T10:00:01Z",
        }
        update_plan = build_fixture_render_plan(context, [old_comment])
        writes: list[object] = []
        changed_comment = {
            "id": 10,
            "body": "changed concurrently",
            "updated_at": "2026-07-11T10:00:02Z",
        }
        with (
            patch.object(module, "verify_live_provenance"),
            patch.object(module, "verify_live_evidence_results"),
            patch.object(
                module, "issue_payload", return_value=context["issue"]
            ),
            patch.object(
                module, "issue_comments", return_value=[changed_comment]
            ),
            patch.object(
                module,
                "gh_api_write",
                side_effect=lambda *args: writes.append(args),
            ),
        ):
            expect_failure(
                lambda: apply_render_plan(
                    "owner/repo", update_plan, {}, {}
                ),
                "managed comment changed concurrently",
            )
        assert writes == []
        assert source_definition_line(
            'const TEXT: &str = "fn exact_test() {}";\n\nfn exact_test() {}\n',
            "tests/exact.rs",
            "exact_test",
        ) == 3
        assert source_definition_line(
            "/* fn exact_test() {} */\n",
            "tests/exact.rs",
            "exact_test",
        ) is None
        assert source_definition_line(
            "# def exact_test():\n\ndef exact_test():\n    pass\n",
            "tests/exact.py",
            "exact_test",
        ) == 3
        malformed_issue = {"body": r"Summary\n\nDetails"}
        trusted_comment = {
            "author_association": "OWNER",
            "body": r"Evidence\n\n- check",
            "html_url": "https://example.invalid/trusted",
        }
        untrusted_comment = {
            "author_association": "NONE",
            "body": r"External\n\ntext",
        }
        assert issue_formatting_failures(
            malformed_issue, [trusted_comment, untrusted_comment], 308
        ) == [
            "#308 body contains unrendered newline escapes",
            "#308 trusted comment https://example.invalid/trusted contains "
            "unrendered newline escapes",
        ]

    def test_2_6() -> None:
        tasks = [
            simple_task("1.1", test_id=None),
            simple_task("1.2", test_id=None),
            simple_task("2.1", test_id=None, section="2"),
        ]
        validate_owner_ranges(
            "change",
            ChangeMapping(
                "checklist-v1", 9, (Owner(9, "1.1", "2.1"),)
            ),
            tasks,
        )
        expect_failure(
            lambda: validate_owner_ranges(
                "change",
                ChangeMapping(
                    "checklist-v1",
                    9,
                    (
                        Owner(9, "1.1", "1.1"),
                        Owner(10, "2.1", "2.1"),
                    ),
                ),
                tasks,
            ),
            "gap",
        )
        expect_failure(
            lambda: validate_owner_ranges(
                "change",
                ChangeMapping(
                    "checklist-v1",
                    9,
                    (
                        Owner(9, "1.1", "1.2"),
                        Owner(10, "1.2", "2.1"),
                    ),
                ),
                tasks,
            ),
            "overlaps",
        )
        expect_failure(
            lambda: validate_owner_ranges(
                "change",
                ChangeMapping(
                    "checklist-v1",
                    9,
                    (
                        Owner(9, "1.1", "1.1"),
                        Owner(9, "1.2", "2.1"),
                    ),
                ),
                tasks,
            ),
            "repeats authoritative issue",
        )
        expect_failure(
            lambda: validate_owner_ranges(
                "change",
                ChangeMapping(
                    "checklist-v1",
                    9,
                    (
                        Owner(10, "2.1", "2.1"),
                        Owner(9, "1.1", "1.2"),
                    ),
                ),
                tasks,
            ),
            "reordered",
        )
        with tempfile.TemporaryDirectory() as directory:
            fixture_root = Path(directory)
            task_path = fixture_root / "openspec/changes/change/tasks.md"
            task_path.parent.mkdir(parents=True)
            task_document = (
                "## 1. Test\n\n"
                "- [ ] 1.1 First\n"
                "- [ ] 1.2 Second\n"
                "\n## 2. Test\n\n"
                "- [ ] 2.1 Third\n"
            )
            task_path.write_text(task_document, encoding="utf-8")
            local = parse_tasks(
                task_document,
                require_test_ids=False,
                source="boundary tasks",
            )
            remote_document = re.sub(
                r"(?m)^## ", "### ", task_document
            )
            suffix = (
                "\n\n## OpenSpec Task Checklist\n\n"
                + remote_document.strip()
                + "\n"
            )

            def issue_body(size: int) -> str:
                filler = size - len(suffix)
                assert filler >= 0
                return "x" * filler + suffix

            mapping = ChangeMapping(
                "checklist-v1", 9, (Owner(9, "1.1", "2.1"),)
            )
            mappings = {"change": mapping}
            task_sets = {"change": local}
            exact = issue_body(BODY_CHARACTER_LIMIT)
            over = issue_body(BODY_CHARACTER_LIMIT + 1)
            with (
                patch.object(
                    module,
                    "issue_payload",
                    return_value={"body": exact, "state": "open"},
                ),
                patch.object(module, "issue_comments", return_value=[]),
            ):
                failures = check_openspec_tasks(
                    "owner/repo",
                    fixture_root,
                    mappings,
                    task_sets,
                    {},
                )
            assert not any("exceeds GitHub" in value for value in failures)
            with (
                patch.object(
                    module,
                    "issue_payload",
                    return_value={"body": over, "state": "open"},
                ),
                patch.object(module, "issue_comments", return_value=[]),
            ):
                failures = check_openspec_tasks(
                    "owner/repo",
                    fixture_root,
                    mappings,
                    task_sets,
                    {},
                )
            assert any("exceeds GitHub" in value for value in failures)
            overhead = hypothetical_primary_length("", task_path)
            prefix = "x" * (BODY_CHARACTER_LIMIT - overhead)
            assert hypothetical_primary_length(prefix, task_path) == BODY_CHARACTER_LIMIT
            assert (
                hypothetical_primary_length(prefix + "x", task_path)
                == BODY_CHARACTER_LIMIT + 1
            )

    def test_2_7() -> None:
        tasks = [
            simple_task("1.1"),
            simple_task("1.2", test_id="TQG-UT-1.2"),
            simple_task("2.1", test_id="TQG-UT-2.1", section="2"),
        ]
        mapping = ChangeMapping(
            "evidence-v2",
            9,
            (
                Owner(9, "1.1", "1.2"),
                Owner(10, "2.1", "2.1"),
            ),
        )
        issues, rows = parse_pr_scope(
            "OpenSpec-Issue: #9\n"
            "OpenSpec-Task: change/1.1..1.2\n"
        )
        scope = expand_pr_scope(
            issues, rows, {"change": mapping}, {"change": tasks}
        )
        assert scope == {"change": {"1.1", "1.2"}}
        expect_failure(
            lambda: expand_pr_scope(
                {10}, rows, {"change": mapping}, {"change": tasks}
            ),
            "does not link authoritative issue",
        )
        crossing = [ScopeRow("change", "1.2", "2.1")]
        expect_failure(
            lambda: expand_pr_scope(
                {9, 10},
                crossing,
                {"change": mapping},
                {"change": tasks},
            ),
            "crosses authority boundaries",
        )
        plan = {
            ("change", "1.1"): simple_plan_row(
                tasks[0], ["src/lib.rs"]
            ),
            ("change", "1.2"): simple_plan_row(
                tasks[1], ["docs/workflow.md"]
            ),
        }
        receipt = (
            "docs/benchmarks/results/phase-0-truth-and-baselines/"
            "task-verification-a95a9de.json"
        )
        assert task_verification_receipt_path(receipt)
        assert not task_verification_receipt_path(
            "docs/benchmarks/results/phase-0-truth-and-baselines/reviews.json"
        )
        assert not task_verification_receipt_path(
            "docs/benchmarks/results/phase-0-truth-and-baselines/"
            "task-verification-a95a9de.txt"
        )
        assert changed_paths_are_owned(
            [
                "src/lib.rs",
                "openspec/changes/change/tasks.md",
                "openspec/task-evidence.json",
                receipt,
            ],
            scope,
            plan,
        ) == []
        failures = changed_paths_are_owned(
            ["Cargo.toml"], scope, plan
        )
        assert failures and "outside declared OpenSpec scope" in failures[0]

        selected: list[set[str] | None] = []

        def record_release_changes(*args, **kwargs) -> list[str]:
            selected.append(kwargs.get("selected_changes"))
            return []

        release_tasks = {
            "first": [
                Task(True, "1.1", "1.1 Done", (), "1")
            ],
            "second": [
                Task(True, "1.1", "1.1 Done", (), "1")
            ],
        }
        release_mappings = {
            "first": ChangeMapping(
                "checklist-v1", 21, (Owner(21, "1.1", "1.1"),)
            ),
            "second": ChangeMapping(
                "checklist-v1", 22, (Owner(22, "1.1", "1.1"),)
            ),
        }
        with (
            patch.object(
                module,
                "milestone_issues",
                return_value=[
                    {"number": 21, "state": "closed"},
                    {"number": 22, "state": "closed"},
                ],
            ),
            patch.object(
                module,
                "check_openspec_tasks",
                side_effect=record_release_changes,
            ),
        ):
            assert check_milestone_complete(
                "owner/repo",
                "v0.4",
                root,
                release_mappings,
                release_tasks,
                {},
            ) == []
        assert selected == [{"first", "second"}]

        provenance = valid_provenance()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "provenance.json"
            write_json(path, provenance)
            assert load_provenance(path, "owner/repo") == provenance
            fork = clone_json(provenance)
            fork["head_repository"] = "fork/repo"
            write_json(path, fork)
            expect_failure(
                lambda: load_provenance(path, "owner/repo"),
                "fork or foreign",
            )

        calls: list[list[str]] = []
        with (
            patch.object(
                module, "gh_api_json", side_effect=live_api(calls=calls)
            ),
            patch.object(
                module,
                "pull_request_payload",
                return_value=valid_pr(),
            ),
        ):
            verify_live_provenance("owner/repo", provenance)
        assert sum("--paginate" in call for call in calls) == 2
        assert sum("--slurp" in call for call in calls) == 2
        assert all(
            call[call.index("--method") + 1] == "GET"
            for call in calls
            if "--paginate" in call
        )

        for field, value, diagnostic in (
            ("run_attempt", 2, "run_attempt"),
            ("head_sha", "b" * 40, "head_sha"),
            ("job_id", 99, "producer job"),
            (
                "artifact_digest",
                "sha256:" + "c" * 64,
                "artifact identity",
            ),
        ):
            hostile = clone_json(provenance)
            hostile[field] = value
            with (
                patch.object(
                    module, "gh_api_json", side_effect=live_api()
                ),
                patch.object(
                    module,
                    "pull_request_payload",
                    return_value=valid_pr(),
                ),
            ):
                expect_failure(
                    lambda hostile=hostile: verify_live_provenance(
                        "owner/repo", hostile
                    ),
                    diagnostic,
                )
        with (
            patch.object(
                module,
                "gh_api_json",
                side_effect=live_api(
                    artifact_updates={"size_in_bytes": MAX_ARTIFACT_BYTES + 1}
                ),
            ),
            patch.object(
                module,
                "pull_request_payload",
                return_value=valid_pr(),
            ),
        ):
            expect_failure(
                lambda: verify_live_provenance("owner/repo", provenance),
                "artifact identity",
            )

        actions_row = {
            "tested_commit": "a" * 40,
            "retained_result": valid_retained_actions(),
        }
        evidence = {
            ("change", "1.1", "TQG-UT-1.1"): actions_row,
            ("change", "1.2", "TQG-UT-1.2"): clone_json(
                actions_row
            ),
        }
        calls = []
        with patch.object(
            module, "gh_api_json", side_effect=live_api(calls=calls)
        ):
            verify_live_evidence_results("owner/repo", evidence)
        assert len(calls) == 3
        individual_hostiles = (
            ("run_id", 9, "mismatch for id"),
            ("run_attempt", 2, "mismatch for run_attempt"),
            ("job_id", 99, "producer job"),
            (
                "artifact_digest",
                "sha256:" + "d" * 64,
                "artifact identity",
            ),
        )
        for field, value, diagnostic in individual_hostiles:
            row = clone_json(actions_row)
            row["retained_result"][field] = value
            with patch.object(
                module, "gh_api_json", side_effect=live_api()
            ):
                expect_failure(
                    lambda row=row: verify_live_evidence_results(
                        "owner/repo",
                        {("change", "1.1", "TQG-UT-1.1"): row},
                    ),
                    diagnostic,
                )
        wrong_sha = clone_json(actions_row)
        wrong_sha["tested_commit"] = "b" * 40
        with patch.object(
            module, "gh_api_json", side_effect=live_api()
        ):
            expect_failure(
                lambda: verify_live_evidence_results(
                    "owner/repo",
                    {("change", "1.1", "TQG-UT-1.1"): wrong_sha},
                ),
                "mismatch for head_sha",
            )

        context = renderer_context()
        old_comment = {
            "id": 10,
            "body": marker_for("change", "1") + "\nold",
            "updated_at": "2026-07-11T10:00:01Z",
        }
        render_plan = build_fixture_render_plan(context, [old_comment])
        events: list[str] = []
        with (
            patch.object(
                module,
                "verify_live_provenance",
                side_effect=lambda *args: events.append("provenance"),
            ),
            patch.object(
                module,
                "verify_live_evidence_results",
                side_effect=lambda *args: events.append("evidence"),
            ),
            patch.object(
                module, "issue_payload", return_value=context["issue"]
            ),
            patch.object(
                module, "issue_comments", return_value=[old_comment]
            ),
            patch.object(
                module,
                "gh_api_write",
                side_effect=lambda *args: events.append("write") or {},
            ),
        ):
            apply_render_plan("owner/repo", render_plan, {}, {})
        assert events[:2] == ["provenance", "evidence"]
        assert events[-1] == "write"

        captured: list[list[str]] = []
        with patch.object(
            module,
            "run",
            side_effect=lambda args, **kwargs: captured.append(args) or "",
        ):
            run_rust_task_validator(
                Path("."),
                "test-quality.toml",
                TQG_TASK_PATH,
                "openspec/task-verification.json",
                "openspec/task-evidence.json",
                "a" * 40,
            )
        assert "--expected-commit" in captured[0]
        assert captured[0][
            captured[0].index("--expected-commit") + 1
        ] == "a" * 40
        expect_failure(
            lambda: run_rust_task_validator(
                Path("."),
                "test-quality.toml",
                TQG_TASK_PATH,
                "openspec/task-verification.json",
                "openspec/task-evidence.json",
                "not-a-commit",
            ),
            "expected commit",
        )

    def test_2_8() -> None:
        cases = [
            require_object(value, "fixture case")
            for value in require_array(fixture["cases"], "cases")
        ]
        for case in cases:
            require_exact_keys(case, {"name", "expected"}, "fixture case")
            require_string(case["name"], "fixture case name")
            if case["expected"] not in {"accept", "reject"}:
                raise AssertionError("fixture case has an invalid expected outcome")
        required_names = {
            "valid-single-issue",
            "valid-managed-comment",
            "valid-phase-ledger",
            "valid-metadata-only-closure",
            "closure-covered-input-drift",
            "aggregate-green-task-evidence-missing",
            "local-only-completion",
            "remote-only-completion",
            "orphan-evidence",
            "stale-github-text",
            "fork-artifact",
            "wrong-run-attempt",
            "wrong-head-sha",
            "wrong-job",
            "wrong-artifact-digest",
            "duplicate-marker",
            "markdown-escaping",
            "pagination",
            "retry-idempotency",
            "concurrency-idempotency",
        }
        case_names = [str(case["name"]) for case in cases]
        assert len(case_names) == len(set(case_names))
        assert set(case_names) == required_names

        first = simple_task("1.1")
        second = simple_task("1.2", test_id="TQG-UT-1.2")
        checked_first = Task(
            True, first.task_id, first.text, first.test_ids, first.section
        )
        context = renderer_context()

        with tempfile.TemporaryDirectory() as directory:
            evidence_values = create_evidence_fixture(directory)
            provenance_path = Path(directory) / "provenance.json"

            def classify(action) -> str:
                try:
                    result = action()
                except ValidationError:
                    return "reject"
                if isinstance(result, list) and result:
                    return "reject"
                return "accept"

            def fork_case() -> object:
                foreign = valid_provenance()
                foreign["head_repository"] = "fork/repo"
                write_json(provenance_path, foreign)
                return load_provenance(provenance_path, "owner/repo")

            def live_case(
                *,
                provenance_updates: dict[str, object] | None = None,
            ) -> None:
                selected = valid_provenance()
                selected.update(provenance_updates or {})
                with (
                    patch.object(
                        module, "gh_api_json", side_effect=live_api()
                    ),
                    patch.object(
                        module,
                        "pull_request_payload",
                        return_value=valid_pr(),
                    ),
                ):
                    verify_live_provenance("owner/repo", selected)

            def orphan_case() -> object:
                payload = clone_json(evidence_values["evidence_payload"])
                payload["results"][0]["change"] = "orphan"
                return load_fixture_evidence(evidence_values, payload)

            def retry_case() -> list[str]:
                existing = {
                    "id": 10,
                    "body": context["desired"],
                    "updated_at": "2026-07-11T10:00:01Z",
                }
                first_plan = build_fixture_render_plan(context, [existing])
                second_plan = build_fixture_render_plan(context, [existing])
                if (
                    canonical_json(first_plan) != canonical_json(second_plan)
                    or first_plan["operations"][0]["action"] != "noop"
                ):
                    return ["renderer retry was not idempotent"]
                return []

            def concurrency_case() -> None:
                old_comment = {
                    "id": 10,
                    "body": marker_for("change", "1") + "\nold",
                    "updated_at": "2026-07-11T10:00:01Z",
                }
                render_plan = build_fixture_render_plan(
                    context, [old_comment]
                )
                changed = {
                    "id": 10,
                    "body": "changed",
                    "updated_at": "2026-07-11T10:00:02Z",
                }
                with (
                    patch.object(module, "verify_live_provenance"),
                    patch.object(module, "verify_live_evidence_results"),
                    patch.object(
                        module,
                        "issue_payload",
                        return_value=context["issue"],
                    ),
                    patch.object(
                        module, "issue_comments", return_value=[changed]
                    ),
                ):
                    apply_render_plan(
                        "owner/repo", render_plan, {}, {}
                    )

            def duplicate_marker_case() -> object:
                marker = marker_for("change", "1")
                duplicate = [
                    {
                        "id": 1,
                        "body": marker + "\nfirst",
                        "updated_at": "one",
                    },
                    {
                        "id": 2,
                        "body": marker + "\nsecond",
                        "updated_at": "two",
                    },
                ]
                return build_fixture_render_plan(context, duplicate)

            def markdown_case() -> list[str]:
                failures: list[str] = []
                for value in require_array(
                    fixture["markdown_cells"], "markdown_cells"
                ):
                    escaped = escape_cell(value)
                    if "<script>" in escaped or "\n" in escaped:
                        failures.append(str(value))
                return failures

            def pagination_case() -> list[str]:
                actual = flatten_paginated_response(fixture["paginated"])
                return (
                    []
                    if actual == [{"number": 1}, {"number": 2}]
                    else ["pagination differs"]
                )

            def managed_comment_case() -> list[str]:
                marker = marker_for("change", "1")
                comments = [{"id": 1, "body": marker + "\nmanaged"}]
                return (
                    []
                    if len(matching_marker_comments(comments, marker)) == 1
                    else ["managed marker differs"]
                )

            def metadata_closure_case() -> list[str]:
                old = "## 1. Test\n- [ ] 1.1 Work `TQG-UT-1.1`\n"
                current = parse_tasks(
                    "## 1. Test\n- [x] 1.1 Work `TQG-UT-1.1`\n",
                    require_test_ids=True,
                    source="closure",
                )
                return validate_task_metadata_transition(
                    "change", old, current, {"1.1"}, True
                )

            def closure_drift_case() -> object:
                source_path = evidence_values["source_path"]
                assert isinstance(source_path, Path)
                original = source_path.read_bytes()
                try:
                    source_path.write_text(
                        "pub fn value() -> u8 { 9 }\n", encoding="utf-8"
                    )
                    payload = clone_json(evidence_values["evidence_payload"])
                    payload["results"][0][
                        "covered_input_digest"
                    ] = covered_input_digest(
                        evidence_values["root"], evidence_values["plan_row"]
                    )
                    return load_fixture_evidence(evidence_values, payload)
                finally:
                    source_path.write_bytes(original)
                    write_json(
                        evidence_values["evidence_path"],
                        evidence_values["evidence_payload"],
                    )

            runners = {
                "valid-single-issue": lambda: validate_owner_ranges(
                    "change",
                    ChangeMapping(
                        "evidence-v2",
                        9,
                        (Owner(9, "1.1", "1.2"),),
                    ),
                    [first, second],
                ),
                "valid-managed-comment": managed_comment_case,
                "valid-phase-ledger": lambda: validate_owner_ranges(
                    "change",
                    ChangeMapping(
                        "evidence-v2",
                        9,
                        (
                            Owner(9, "1.1", "1.1"),
                            Owner(10, "1.2", "1.2"),
                        ),
                    ),
                    [first, second],
                ),
                "valid-metadata-only-closure": metadata_closure_case,
                "closure-covered-input-drift": closure_drift_case,
                "aggregate-green-task-evidence-missing": lambda: checked_evidence_failures(
                    "change", [checked_first], {}
                ),
                "local-only-completion": lambda: compare_task_sequences(
                    [checked_first], [first], "local only"
                ),
                "remote-only-completion": lambda: compare_task_sequences(
                    [first], [checked_first], "remote only"
                ),
                "orphan-evidence": orphan_case,
                "stale-github-text": lambda: compare_task_sequences(
                    [first],
                    [
                        simple_task(
                            "1.1",
                            text="Stale GitHub text",
                            test_id="TQG-UT-1.1",
                        )
                    ],
                    "stale GitHub",
                ),
                "fork-artifact": fork_case,
                "wrong-run-attempt": lambda: live_case(
                    provenance_updates={"run_attempt": 2}
                ),
                "wrong-head-sha": lambda: live_case(
                    provenance_updates={"head_sha": "b" * 40}
                ),
                "wrong-job": lambda: live_case(
                    provenance_updates={"job_id": 99}
                ),
                "wrong-artifact-digest": lambda: live_case(
                    provenance_updates={
                        "artifact_digest": "sha256:" + "d" * 64
                    }
                ),
                "duplicate-marker": duplicate_marker_case,
                "markdown-escaping": markdown_case,
                "pagination": pagination_case,
                "retry-idempotency": retry_case,
                "concurrency-idempotency": concurrency_case,
            }
            executed: set[str] = set()
            for case in cases:
                name = str(case["name"])
                actual = classify(runners[name])
                assert actual == case["expected"], (
                    f"fixture {name} expected {case['expected']} but got {actual}"
                )
                executed.add(name)
            assert executed == required_names

    tests = {
        "TQG-UT-2.1": test_2_1,
        "TQG-UT-2.2": test_2_2,
        "TQG-UT-2.3": test_2_3,
        "TQG-UT-2.4": test_2_4,
        "TQG-UT-2.5": test_2_5,
        "TQG-UT-2.6": test_2_6,
        "TQG-UT-2.7": test_2_7,
        "TQG-UT-2.8": test_2_8,
    }
    selected = [only_test_id] if only_test_id else list(tests)
    for test_id in selected:
        if test_id not in tests:
            raise ValidationError(f"unknown self-test id {test_id}")
        tests[test_id]()
        print(f"{test_id} passed")



def source_digests(paths: dict[str, Path]) -> dict[str, str]:
    """Digest the exact checked-in inputs used to build a render plan."""

    return {name: sha256_digest(path.read_bytes()) for name, path in sorted(paths.items())}


def final_closeout_failures(
    repo: str,
    root: Path,
    mappings: dict[str, ChangeMapping],
    task_sets: dict[str, list[Task]],
    plan: dict[tuple[str, str], dict[str, object]],
    evidence: dict[tuple[str, str, str], dict[str, object]],
    expected_commit: str,
    digests: dict[str, str],
) -> list[str]:
    """Require exact-final-SHA hosted evidence and already-rendered permalinks."""

    failures: list[str] = []
    if not SHA_RE.fullmatch(expected_commit):
        return ["--expected-commit must be a canonical lowercase commit SHA"]
    head = git_output(
        root,
        ["rev-parse", "--verify", "HEAD^{commit}"],
        "cannot resolve repository HEAD",
    )
    if head != expected_commit:
        failures.append(
            f"final closeout commit {expected_commit} is not checkout HEAD {head}"
        )
    for change in sorted(FINAL_VERIFICATION_CHANGES):
        mapping = mappings.get(change)
        if mapping is None or mapping.contract != "evidence-v2":
            failures.append(f"final verification change is not evidence-v2: {change}")
            continue
        for task in task_sets[change]:
            if not task.checked:
                failures.append(f"{change}/{task.task_id} is incomplete at final closeout")
                continue
            row = plan[(change, task.task_id)]
            for test_id in task.test_ids:
                result = evidence.get((change, task.task_id, test_id))
                if result is None:
                    failures.append(
                        f"{change}/{task.task_id} lacks final evidence for {test_id}"
                    )
                    continue
                if result["tested_commit"] != expected_commit:
                    failures.append(
                        f"{change}/{task.task_id}/{test_id} is not bound to "
                        f"{expected_commit}"
                    )
                retained = require_object(result["retained_result"], "retained_result")
                if retained["kind"] != "github_actions":
                    failures.append(
                        f"{change}/{task.task_id}/{test_id} lacks hosted Actions evidence"
                    )
                if test_source_for(row, test_id) is None:
                    failures.append(
                        f"{change}/{task.task_id}/{test_id} lacks exact test source metadata"
                    )
                elif not derived_test_link(root, repo, row, result, test_id).startswith("["):
                    failures.append(
                        f"{change}/{task.task_id}/{test_id} test source does not resolve "
                        f"at {expected_commit}"
                    )
    if failures:
        return failures
    verify_live_evidence_results(repo, evidence)
    rendered = build_render_plan(
        repo, root, mappings, task_sets, plan, evidence, digests
    )
    for value in require_array(rendered["operations"], "operations"):
        operation = require_object(value, "operation")
        if operation["action"] != "noop":
            failures.append(
                f"#{operation['issue']} final evidence comment {operation['marker']} "
                f"requires {operation['action']}"
            )
    return failures


def main() -> None:
    """Parse CLI arguments and run the requested read or narrowly validated write mode."""

    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="")
    parser.add_argument("--root", default=".")
    parser.add_argument("--issue-map", default="openspec/issue-map.json")
    parser.add_argument("--verification-plan", default="openspec/task-verification.json")
    parser.add_argument("--evidence", default="openspec/task-evidence.json")
    parser.add_argument("--policy", default="test-quality.toml")
    parser.add_argument("--milestone", action="append", default=[])
    parser.add_argument("--pr", type=int)
    parser.add_argument("--render-plan")
    parser.add_argument("--apply-render-plan")
    parser.add_argument("--provenance")
    parser.add_argument("--rust-task-validator", action="store_true")
    parser.add_argument("--require-run-links", action="store_true")
    parser.add_argument("--expected-commit", default="")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--test-id")
    args = parser.parse_args()

    root = Path(args.root)
    if args.self_test:
        self_test(root, args.test_id)
        return
    if args.test_id:
        raise ValidationError("--test-id requires --self-test")
    if args.expected_commit and not (
        args.rust_task_validator or args.require_run_links
    ):
        raise ValidationError(
            "--expected-commit requires --rust-task-validator or --require-run-links"
        )
    if args.require_run_links and not args.expected_commit:
        raise ValidationError("--require-run-links requires --expected-commit")
    if not args.repo:
        raise ValidationError("--repo is required unless --self-test is used")
    repo_parts(args.repo)

    issue_map_path = root / args.issue_map
    verification_path = root / args.verification_plan
    evidence_path = root / args.evidence
    mappings, task_sets = load_issue_map(issue_map_path, root)
    plan, _ = load_verification_plan(verification_path, mappings, task_sets)
    evidence, _ = load_evidence(
        evidence_path, root, mappings, task_sets, plan
    )
    failures: list[str] = []
    if args.pr:
        pr_failures, selected_changes = check_pr_scope(
            args.repo, args.pr, root, mappings, task_sets, plan, evidence
        )
        failures.extend(pr_failures)
        failures.extend(
            check_openspec_tasks(
                args.repo,
                root,
                mappings,
                task_sets,
                evidence,
                selected_changes=selected_changes,
            )
        )
    else:
        failures.extend(
            check_openspec_tasks(args.repo, root, mappings, task_sets, evidence)
        )
    for milestone in args.milestone:
        failures.extend(
            check_milestone_complete(
                args.repo, milestone, root, mappings, task_sets, evidence
            )
        )
    if args.rust_task_validator:
        expected_commit = args.expected_commit or git_output(
            root,
            ["rev-parse", "--verify", "HEAD^{commit}"],
            "cannot resolve expected commit",
        )
        if not SHA_RE.fullmatch(expected_commit):
            raise ValidationError("--expected-commit must be a canonical commit SHA")
        # The typed validator accepts one task file per call; run once per v2 change.
        for change, mapping in mappings.items():
            if mapping.contract == "evidence-v2":
                run_rust_task_validator(
                    root,
                    args.policy,
                    f"openspec/changes/{change}/tasks.md",
                    args.verification_plan,
                    args.evidence,
                    expected_commit,
                )

    digest_paths = {
        "issue_map": issue_map_path,
        "verification_plan": verification_path,
        "evidence": evidence_path,
    }
    digests = source_digests(digest_paths)
    if args.require_run_links:
        failures.extend(
            final_closeout_failures(
                args.repo,
                root,
                mappings,
                task_sets,
                plan,
                evidence,
                args.expected_commit,
                digests,
            )
        )
    if args.render_plan or args.apply_render_plan:
        if not args.provenance:
            failures.append("renderer planning/apply requires --provenance")
        else:
            provenance = load_provenance(root / args.provenance, args.repo)
            actions_cache: dict[str, dict[object, object]] = {}
            verify_live_provenance(args.repo, provenance, actions_cache)
            verify_live_evidence_results(args.repo, evidence, actions_cache)
            fresh_plan = build_render_plan(
                args.repo,
                root,
                mappings,
                task_sets,
                plan,
                evidence,
                digests,
            )
            if args.render_plan:
                Path(args.render_plan).write_text(
                    json.dumps(fresh_plan, indent=2, ensure_ascii=True) + "\n",
                    encoding="utf-8",
                )
            if args.apply_render_plan:
                supplied = validate_render_plan(
                    json.loads(Path(args.apply_render_plan).read_text(encoding="utf-8")),
                    args.repo,
                )
                if canonical_json(supplied) != canonical_json(fresh_plan):
                    failures.append("render plan is stale or differs from fresh validated state")
                else:
                    apply_render_plan(args.repo, fresh_plan, provenance, evidence)

    if failures:
        print("\nIssue checklist validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    try:
        main()
    except ValidationError as error:
        print(f"Issue checklist validation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
