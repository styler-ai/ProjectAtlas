"""Purpose: Verify GitHub issue checklists mirror OpenSpec tasks before release."""

import argparse
import ast
import base64
import hashlib
import io
import json
import re
import subprocess
import sys
import zipfile
from pathlib import Path
from urllib.parse import quote, unquote


TASK_RE = re.compile(r"(?m)^\s*[-*]\s+\[([ xX])\]\s+(.+?)\s*$")
HEADING_RE = re.compile(r"(?m)^(#{1,6})\s+(.+?)\s*$")
TASK_SECTION_HEADING_RE = re.compile(r"^\d+(?:\.\d+)*\.?\s+")
TASK_ID_RE = re.compile(r"^(\d+(?:\.\d+)*)\s+")
MARKDOWN_LINK_RE = re.compile(r"\[([^\]\r\n]+)\]\((https://github\.com/[^)\s]+)\)")
RUN_URL_RE = re.compile(
    r"^https://github\.com/([^/]+)/([^/]+)/actions/runs/(\d+)(?:/.*)?$"
)
TEST_BLOB_URL_RE = re.compile(
    r"^https://github\.com/([^/]+)/([^/]+)/blob/([0-9a-fA-F]{40})/(.+)#L(\d+)(?:-L(\d+))?$"
)
COMMIT_RE = re.compile(r"^[0-9a-fA-F]{40}$")
SHA256_RE = re.compile(r"^[0-9a-fA-F]{64}$")
INLINE_CODE_RE = re.compile(r"`[^`\r\n]*`")
TRUSTED_COMMENT_ASSOCIATIONS = {"OWNER", "MEMBER", "COLLABORATOR"}
COMMAND_TIMEOUT_SECONDS = 60
TASK_EVIDENCE_FORMAT = "projectatlas.task-evidence.v1"
TASK_EVIDENCE_FILENAME = "projectatlas-task-evidence.json"
MAX_TASK_EVIDENCE_ARCHIVE_BYTES = 2_000_000
UNIT_TEST_EXEMPTION_SCOPE = "unit-test-link-only"
FINAL_VERIFICATION_CHANGES = {
    "advance-rust-repository-intelligence",
    "enforce-rust-test-quality-gates",
}
FINAL_EVIDENCE_POLICY = {
    "workflow_name": "04-Repository-Quality-Evidence",
    "workflow_path": ".github/workflows/repository-quality-evidence.yml",
    "job_name": "task-evidence",
    "step_name": "Run task verification plan",
    "artifact_name": "projectatlas-task-evidence",
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
SUPPORTED_TEST_SOURCE_SUFFIXES = tuple(SOURCE_DEFINITION_PATTERNS)


def run(args):
    try:
        process = subprocess.run(
            args, capture_output=True, text=True, timeout=COMMAND_TIMEOUT_SECONDS
        )
    except subprocess.TimeoutExpired as error:
        raise SystemExit(
            f"command timed out after {COMMAND_TIMEOUT_SECONDS}s: {' '.join(args)}"
        ) from error
    if process.returncode:
        raise SystemExit(
            f"command failed: {' '.join(args)}\n{process.stderr.strip()}"
        )
    return process.stdout


def run_binary(args):
    try:
        process = subprocess.run(
            args, capture_output=True, timeout=COMMAND_TIMEOUT_SECONDS
        )
    except subprocess.TimeoutExpired as error:
        raise SystemExit(
            f"command timed out after {COMMAND_TIMEOUT_SECONDS}s: {' '.join(args)}"
        ) from error
    if process.returncode:
        message = process.stderr.decode("utf-8", errors="replace").strip()
        raise SystemExit(f"command failed: {' '.join(args)}\n{message}")
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
            "body,comments,title,state,url",
        ]
    )


def issue_checklist_tasks(issue, heading_predicate):
    tasks = []
    tasks.extend(parse_section_tasks(issue.get("body", ""), heading_predicate))
    return tasks


def issue_task_blocks(issue, heading_predicate):
    text = issue.get("body", "") or ""
    blocks = {}
    headings = list(HEADING_RE.finditer(text))
    for index, heading in enumerate(headings):
        if not heading_predicate(heading.group(2)):
            continue
        level = len(heading.group(1))
        end = len(text)
        for next_heading in headings[index + 1 :]:
            if len(next_heading.group(1)) <= level and not heading_is_task_subsection(
                next_heading.group(2)
            ):
                end = next_heading.start()
                break
        section = text[heading.end() : end]
        matches = list(TASK_RE.finditer(section))
        for task_index, match in enumerate(matches):
            block_end = (
                matches[task_index + 1].start()
                if task_index + 1 < len(matches)
                else len(section)
            )
            task = clean(match.group(2))
            blocks.setdefault(task, []).append(section[match.start() : block_end])
    return blocks


def implemented_test_contract_is_valid(unit_test):
    command = unit_test.get("command")
    assertion = unit_test.get("assertion")
    return (
        isinstance(unit_test.get("test_id"), str)
        and bool(unit_test["test_id"].strip())
        and isinstance(unit_test.get("function"), str)
        and bool(unit_test["function"].strip())
        and isinstance(command, dict)
        and set(command) == {"executable", "arguments"}
        and isinstance(command["executable"], str)
        and bool(command["executable"].strip())
        and isinstance(command["arguments"], list)
        and bool(command["arguments"])
        and all(
            isinstance(argument, str) and bool(argument)
            for argument in command["arguments"]
        )
        and isinstance(assertion, str)
        and bool(assertion.strip())
    )


def load_test_link_policy(path):
    with open(path, encoding="utf-8") as handle:
        plan = json.load(handle)
    return test_link_policy(plan, path)


def test_link_policy(plan, source):
    requirements = {}
    for source in ("stable_row_overrides", "planned_row_definitions"):
        for row in plan.get(source, []):
            unit_test = row.get("unit_test", {})
            state = unit_test.get("state", "")
            function = unit_test.get("function")
            test_id = unit_test.get("test_id")
            if not state.startswith("implemented"):
                continue
            command = unit_test.get("command")
            assertion = unit_test.get("assertion")
            if not implemented_test_contract_is_valid(unit_test):
                raise SystemExit(
                    f"{source} has an invalid implemented test command/assertion for "
                    f"{row.get('change')}:{row.get('task_id')}"
                )
            identity = (row.get("change"), row.get("task_id"))
            if identity in requirements:
                raise SystemExit(
                    f"{source} has duplicate implemented unit-test rows for {identity[0]}:{identity[1]}"
                )
            source_path = unit_test.get("source_path")
            if not source_path:
                module = function.split("::", 1)[0]
                candidates = [
                    artifact
                    for artifact in row.get("changed_artifacts", [])
                    if artifact.endswith(SUPPORTED_TEST_SOURCE_SUFFIXES)
                    and Path(artifact).stem == module
                ]
                if len(candidates) != 1:
                    raise SystemExit(
                        f"{source} cannot resolve one test source for {identity[0]}:{identity[1]}"
                    )
                source_path = candidates[0]
            source_anchor = unit_test.get("source_anchor") or function.rsplit("::", 1)[-1]
            if (
                not isinstance(source_path, str)
                or not source_path.strip()
                or Path(source_path).suffix.lower() not in SOURCE_DEFINITION_PATTERNS
                or not isinstance(source_anchor, str)
                or not source_anchor.strip()
            ):
                raise SystemExit(
                    f"{source} has an invalid test source path/anchor for "
                    f"{identity[0]}:{identity[1]}"
                )
            requirements[identity] = {
                "test_id": test_id,
                "function": function,
                "command": command,
                "assertion": assertion,
                "source_path": source_path.replace("\\", "/"),
                "source_anchor": source_anchor,
                "result": row.get("result", {}),
            }
    exemptions = {}
    for exemption in plan.get("unit_test_exemptions", []):
        if not isinstance(exemption, dict) or set(exemption) != {
            "change",
            "task_id",
            "scope",
            "reason",
        }:
            raise SystemExit(f"{source} has a malformed unit-test exemption")
        identity = (exemption["change"], exemption["task_id"])
        reason = exemption["reason"]
        if (
            not all(isinstance(value, str) and value.strip() for value in identity)
            or exemption["scope"] != UNIT_TEST_EXEMPTION_SCOPE
            or not isinstance(reason, str)
            or not reason.strip()
        ):
            raise SystemExit(
                f"{source} has an invalid unit-test exemption for {identity[0]}:{identity[1]}"
            )
        if identity in requirements or identity in exemptions:
            raise SystemExit(
                f"{source} has overlapping or duplicate test policy for {identity[0]}:{identity[1]}"
            )
        exemptions[identity] = reason.strip()
    return requirements, exemptions


def verification_plan_changes(plan, source):
    changes = [entry.get("change") for entry in plan.get("task_sources", [])]
    if not changes or any(not isinstance(change, str) or not change for change in changes):
        raise SystemExit(f"{source} must declare non-empty task_sources change names")
    if len(changes) != len(set(changes)):
        raise SystemExit(f"{source} has duplicate task_sources change names")
    change_set = set(changes)
    if change_set != FINAL_VERIFICATION_CHANGES:
        raise SystemExit(
            f"{source} final task_sources must be exactly "
            f"{sorted(FINAL_VERIFICATION_CHANGES)}, got {sorted(change_set)}"
        )
    return change_set


def load_verification_plan_changes(path):
    with open(path, encoding="utf-8") as handle:
        plan = json.load(handle)
    return verification_plan_changes(plan, path)


def task_id(task):
    match = TASK_ID_RE.match(task)
    return match.group(1) if match else None


def github_run_id(url, repo):
    match = RUN_URL_RE.match(url)
    if not match or f"{match.group(1)}/{match.group(2)}".lower() != repo.lower():
        return None
    return int(match.group(3))


def run_payload_is_successful(payload, expected_commit):
    return (
        payload.get("status") == "completed"
        and payload.get("conclusion") == "success"
        and payload.get("head_sha", "").lower() == expected_commit.lower()
    )


def exact_test_link_is_valid(repo, url, expected_commit, requirement, cache):
    match = TEST_BLOB_URL_RE.match(url)
    linked_path = unquote(match.group(4)) if match else ""
    if (
        not match
        or f"{match.group(1)}/{match.group(2)}".lower() != repo.lower()
        or match.group(3).lower() != expected_commit.lower()
        or linked_path != requirement["source_path"]
    ):
        return False
    cache_key = (match.group(3).lower(), linked_path)
    if cache_key not in cache:
        encoded_path = quote(linked_path, safe="/")
        payload = gh_api_json(
            [f"repos/{repo}/contents/{encoded_path}?ref={match.group(3)}"]
        )
        if payload.get("encoding") != "base64":
            return False
        cache[cache_key] = base64.b64decode(payload.get("content", "")).decode("utf-8")
    start = int(match.group(5))
    end = int(match.group(6) or match.group(5))
    source = cache[cache_key]
    lines = source.splitlines()
    if start < 1 or end != start or end > len(lines):
        return False
    return source_definition_is_at_line(
        source, linked_path, requirement["source_anchor"], start
    )


def c_like_code_lines(source):
    output = []
    current = []
    index = 0
    block_comment_depth = 0
    quote_character = None
    raw_terminator = None
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
        raw_match = re.match(r"(?:br|r)(?P<hashes>#{0,255})\"", source[index:])
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


def source_definition_is_at_line(source, source_path, anchor, line):
    suffix = Path(source_path).suffix.lower()
    if suffix == ".py":
        try:
            tree = ast.parse(source)
        except SyntaxError:
            return False
        return any(
            isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and node.name == anchor
            and node.lineno == line
            for node in ast.walk(tree)
        )
    pattern_template = SOURCE_DEFINITION_PATTERNS.get(suffix)
    if pattern_template is None:
        return False
    code_lines = (
        c_like_code_lines(source)
        if suffix in {".rs", ".js", ".jsx", ".ts", ".tsx"}
        else source.splitlines()
    )
    if line > len(code_lines):
        return False
    pattern = pattern_template.format(anchor=re.escape(anchor))
    candidate = code_lines[line - 1]
    if suffix in {".ps1", ".sh"} and candidate.lstrip().startswith("#"):
        return False
    return re.search(pattern, candidate) is not None


def canonical_sha256(value):
    encoded = json.dumps(
        value, ensure_ascii=False, separators=(",", ":"), sort_keys=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def download_task_evidence_manifest(repo, artifact_id):
    archive = run_binary(
        ["gh", "api", f"repos/{repo}/actions/artifacts/{artifact_id}/zip"]
    )
    if len(archive) > MAX_TASK_EVIDENCE_ARCHIVE_BYTES:
        raise SystemExit(
            f"task evidence artifact {artifact_id} exceeds "
            f"{MAX_TASK_EVIDENCE_ARCHIVE_BYTES} bytes"
        )
    try:
        with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
            entries = [
                entry
                for entry in bundle.infolist()
                if entry.filename == TASK_EVIDENCE_FILENAME and not entry.is_dir()
            ]
            if len(entries) != 1:
                return None
            entry = entries[0]
            if entry.file_size > MAX_TASK_EVIDENCE_ARCHIVE_BYTES:
                return None
            return json.loads(bundle.read(entry).decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError, zipfile.BadZipFile):
        return None


def task_evidence_manifest_is_valid(
    requirement, repo, run_id, expected_commit, run_attempt, manifest
):
    if not isinstance(manifest, dict) or set(manifest) != {
        "format",
        "repository",
        "commit_sha",
        "run_id",
        "run_attempt",
        "tests",
    }:
        return False
    if (
        manifest["format"] != TASK_EVIDENCE_FORMAT
        or manifest["repository"].lower() != repo.lower()
        or manifest["commit_sha"].lower() != expected_commit.lower()
        or manifest["run_id"] != run_id
        or manifest["run_attempt"] != run_attempt
        or not isinstance(manifest["tests"], list)
    ):
        return False
    expected_test = {
        "test_id": requirement["test_id"],
        "state": "passed",
        "command_digest": canonical_sha256(requirement.get("command")),
        "assertion_digest": canonical_sha256(requirement.get("assertion")),
        "covered_input_digest": requirement.get("result", {}).get(
            "covered_input_digest"
        ),
        "implementation_commit": expected_commit,
    }
    matches = [
        entry
        for entry in manifest["tests"]
        if isinstance(entry, dict) and entry.get("test_id") == requirement["test_id"]
    ]
    return len(matches) == 1 and matches[0] == expected_test


def hosted_evidence_payload_is_successful(
    requirement,
    repo,
    run_id,
    expected_commit,
    run_payload,
    jobs_payload,
    artifacts_payload,
    artifact_manifests,
):
    result = requirement.get("result", {})
    hosted = result.get("hosted_evidence")
    if not isinstance(hosted, dict):
        return False
    required = {
        "repository",
        "test_id",
        "run_id",
        "run_attempt",
        "workflow_name",
        "workflow_path",
        "job_id",
        "job_name",
        "step_name",
        "artifact_name",
        "artifact_digest",
        "commit_sha",
    }
    if set(hosted) != required:
        return False
    expected_identity = (
        f"github-actions:{run_id}:{hosted['run_attempt']}:{hosted['job_name']}"
    )
    if (
        hosted["repository"].lower() != repo.lower()
        or hosted["test_id"] != requirement["test_id"]
        or hosted["run_id"] != run_id
        or hosted["commit_sha"].lower() != expected_commit.lower()
        or any(
            hosted[field] != expected
            for field, expected in FINAL_EVIDENCE_POLICY.items()
        )
        or run_payload.get("name") != hosted["workflow_name"]
        or run_payload.get("path") != hosted["workflow_path"]
        or result.get("implementation_commit", "").lower() != expected_commit.lower()
        or result.get("run_identity") != expected_identity
        or result.get("artifact_digest") != hosted["artifact_digest"]
        or not SHA256_RE.fullmatch(result.get("covered_input_digest", ""))
        or not SHA256_RE.fullmatch(hosted["artifact_digest"])
        or not run_payload_is_successful(run_payload, expected_commit)
        or run_payload.get("run_attempt") != hosted["run_attempt"]
    ):
        return False
    job_matches = [
        job
        for job in jobs_payload.get("jobs", [])
        if job.get("id") == hosted["job_id"]
        and job.get("name") == hosted["job_name"]
        and job.get("status") == "completed"
        and job.get("conclusion") == "success"
        and job.get("run_id") == run_id
        and job.get("run_attempt") == hosted["run_attempt"]
        and job.get("head_sha", "").lower() == expected_commit.lower()
    ]
    if len(job_matches) != 1:
        return False
    step_matches = [
        step
        for step in job_matches[0].get("steps", [])
        if step.get("name") == hosted["step_name"]
        and step.get("status") == "completed"
        and step.get("conclusion") == "success"
    ]
    artifact_matches = [
        artifact
        for artifact in artifacts_payload.get("artifacts", [])
        if artifact.get("name") == hosted["artifact_name"]
        and artifact.get("digest") == f"sha256:{hosted['artifact_digest']}"
        and not artifact.get("expired", True)
        and artifact.get("workflow_run", {}).get("id") == run_id
        and artifact.get("workflow_run", {}).get("head_sha", "").lower()
        == expected_commit.lower()
    ]
    if len(step_matches) != 1 or len(artifact_matches) != 1:
        return False
    artifact_id = artifact_matches[0].get("id")
    return isinstance(artifact_id, int) and task_evidence_manifest_is_valid(
        requirement,
        repo,
        run_id,
        expected_commit,
        hosted["run_attempt"],
        artifact_manifests.get(artifact_id),
    )


def github_run_evidence(repo, run_id, cache):
    if run_id not in cache:
        cache[run_id] = {
            "run": gh_api_json([f"repos/{repo}/actions/runs/{run_id}"]),
            "jobs": merge_paginated_objects(
                gh_api_json(
                    [
                        "--paginate",
                        "--slurp",
                        f"repos/{repo}/actions/runs/{run_id}/jobs?per_page=100",
                    ]
                ),
                "jobs",
            ),
            "artifacts": merge_paginated_objects(
                gh_api_json(
                    [
                        "--paginate",
                        "--slurp",
                        f"repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100",
                    ]
                ),
                "artifacts",
            ),
            "artifact_manifests": {},
        }
    return cache[run_id]


def github_run_is_successful(repo, url, expected_commit, requirement, cache):
    run_id = github_run_id(url, repo)
    if run_id is None:
        return False
    evidence = github_run_evidence(repo, run_id, cache)
    manifests = evidence.setdefault("artifact_manifests", {})
    hosted = requirement.get("result", {}).get("hosted_evidence", {})
    for artifact in evidence["artifacts"].get("artifacts", []):
        if (
            artifact.get("name") == hosted.get("artifact_name")
            and isinstance(artifact.get("id"), int)
            and artifact["id"] not in manifests
        ):
            manifests[artifact["id"]] = download_task_evidence_manifest(
                repo, artifact["id"]
            )
    return hosted_evidence_payload_is_successful(
        requirement,
        repo,
        run_id,
        expected_commit,
        evidence["run"],
        evidence["jobs"],
        evidence["artifacts"],
        manifests,
    )


def github_run_is_successful_for_exemption(repo, url, expected_commit, cache):
    run_id = github_run_id(url, repo)
    if run_id is None:
        return False
    evidence = github_run_evidence(repo, run_id, cache)
    run_payload = evidence["run"]
    if (
        not run_payload_is_successful(run_payload, expected_commit)
        or run_payload.get("name") != FINAL_EVIDENCE_POLICY["workflow_name"]
        or run_payload.get("path") != FINAL_EVIDENCE_POLICY["workflow_path"]
    ):
        return False
    jobs = [
        job
        for job in evidence["jobs"].get("jobs", [])
        if job.get("name") == FINAL_EVIDENCE_POLICY["job_name"]
        and job.get("status") == "completed"
        and job.get("conclusion") == "success"
        and job.get("run_id") == run_id
        and job.get("run_attempt") == run_payload.get("run_attempt")
        and job.get("head_sha", "").lower() == expected_commit.lower()
    ]
    return len(jobs) == 1 and sum(
        step.get("name") == FINAL_EVIDENCE_POLICY["step_name"]
        and step.get("status") == "completed"
        and step.get("conclusion") == "success"
        for step in jobs[0].get("steps", [])
    ) == 1


def issue_evidence_link_failures(
    issue,
    issue_number,
    change,
    expected_tasks,
    requirements,
    exemptions,
    repo,
    enforce_test_policy,
    require_run_links,
    expected_commit,
    run_cache,
    source_cache,
):
    failures = []
    blocks = issue_task_blocks(issue, heading_matches_openspec_tasks)
    repository_url = f"https://github.com/{repo}/"
    run_url = f"{repository_url}actions/runs/"
    for checked, task in expected_tasks:
        if not checked:
            if require_run_links:
                failures.append(
                    f"#{issue_number} task {task_id(task)} is incomplete at final closeout"
                )
            continue
        current_task_id = task_id(task)
        identity = (change, current_task_id)
        task_blocks = blocks.get(task, [])
        requirement = requirements.get(identity)
        exemption = exemptions.get(identity)
        links = [
            (label, url)
            for block in task_blocks
            for label, url in MARKDOWN_LINK_RE.findall(block)
        ]
        if enforce_test_policy and requirement is None and exemption is None:
            failures.append(
                f"#{issue_number} checked task {current_task_id} lacks an implemented "
                "test definition or explicit unit-test exemption"
            )
        if requirement:
            has_unit_link = any(
                label == requirement["test_id"] and url.startswith(repository_url)
                for label, url in links
            )
            if not has_unit_link:
                failures.append(
                    f"#{issue_number} checked task {task_id(task)} lacks clickable "
                    f"test link {requirement['test_id']}"
                )
            elif require_run_links and not any(
                label == requirement["test_id"]
                and exact_test_link_is_valid(
                    repo, url, expected_commit, requirement, source_cache
                )
                for label, url in links
            ):
                failures.append(
                    f"#{issue_number} checked task {task_id(task)} lacks an exact-final-SHA "
                    f"test permalink for {requirement['test_id']}"
                )
        if require_run_links:
            run_links = [url for _, url in links if url.startswith(run_url)]
            if not run_links:
                failures.append(
                    f"#{issue_number} checked task {task_id(task)} lacks final GitHub run link"
                )
            elif requirement is not None:
                successful_run = any(
                    github_run_is_successful(
                        repo, url, expected_commit, requirement, run_cache
                    )
                    for url in run_links
                )
            elif exemption is not None:
                successful_run = any(
                    github_run_is_successful_for_exemption(
                        repo, url, expected_commit, run_cache
                    )
                    for url in run_links
                )
            else:
                successful_run = False
            if run_links and not successful_run:
                failures.append(
                    f"#{issue_number} checked task {task_id(task)} has no successful GitHub "
                    f"run for {expected_commit}"
                )
    return failures


def has_unrendered_newline_escapes(text):
    escaped_newlines = 0
    fence = None
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


def issue_formatting_failures(issue, issue_number):
    failures = []
    if has_unrendered_newline_escapes(issue.get("body", "")):
        failures.append(f"#{issue_number} body contains unrendered newline escapes")
    for comment in issue.get("comments", []):
        if comment.get("authorAssociation") not in TRUSTED_COMMENT_ASSOCIATIONS:
            continue
        if has_unrendered_newline_escapes(comment.get("body", "")):
            location = comment.get("url") or comment.get("id") or "unknown comment"
            failures.append(
                f"#{issue_number} trusted comment {location} contains unrendered newline escapes"
            )
    return failures


def local_tasks(root, change):
    path = root / "openspec" / "changes" / change / "tasks.md"
    if not path.exists():
        raise SystemExit(f"OpenSpec tasks file missing for {change}: {path}")
    tasks = parse_tasks(path.read_text(encoding="utf-8"))
    if not tasks:
        raise SystemExit(f"OpenSpec tasks file has no checkbox tasks: {path}")
    return path, tasks


def first_task_mismatch(expected_tasks, remote_tasks):
    for index in range(max(len(expected_tasks), len(remote_tasks))):
        expected = expected_tasks[index] if index < len(expected_tasks) else None
        remote = remote_tasks[index] if index < len(remote_tasks) else None
        if expected != remote:
            return index, expected, remote
    return None


def repository_head(root):
    head = run(["git", "-C", str(root), "rev-parse", "HEAD"]).strip()
    if not COMMIT_RE.fullmatch(head):
        raise SystemExit(f"git rev-parse HEAD returned an invalid commit: {head!r}")
    return head.lower()


def expected_commit_is_checkout(expected_commit, head_commit):
    return expected_commit.lower() == head_commit.lower()


def unknown_test_policy_identities(change, expected_tasks, requirements, exemptions):
    expected = {(change, task_id(task)) for _, task in expected_tasks}
    return sorted(
        identity
        for identity in set(requirements) | set(exemptions)
        if identity[0] == change and identity not in expected
    )


def check_openspec_tasks(
    repo,
    root,
    issue_map,
    test_requirements,
    test_exemptions,
    require_run_links=False,
    expected_commit="",
    final_changes=None,
):
    failures = []
    run_cache = {}
    source_cache = {}
    for change, issue_number in sorted(issue_map.items()):
        if require_run_links and change not in (final_changes or set()):
            continue
        path, expected_tasks = local_tasks(root, change)
        for policy_identity in unknown_test_policy_identities(
            change, expected_tasks, test_requirements, test_exemptions
        ):
            failures.append(
                f"{path} has test policy for unknown task {policy_identity[0]}:{policy_identity[1]}"
            )
        issue = issue_payload(repo, issue_number)
        failures.extend(issue_formatting_failures(issue, issue_number))
        failures.extend(
            issue_evidence_link_failures(
                issue,
                issue_number,
                change,
                expected_tasks,
                test_requirements,
                test_exemptions,
                repo,
                change in (final_changes or set()),
                require_run_links,
                expected_commit,
                run_cache,
                source_cache,
            )
        )
        remote_tasks = issue_checklist_tasks(issue, heading_matches_openspec_tasks)
        mismatch = first_task_mismatch(expected_tasks, remote_tasks)
        if mismatch is not None:
            failures.append(
                f"#{issue_number} checklist order/state/text differs from {path} at "
                f"position {mismatch[0] + 1}: expected={mismatch[1]!r}, remote={mismatch[2]!r}"
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


def merge_paginated_objects(payload, collection_key):
    if isinstance(payload, dict):
        pages = [payload]
    elif isinstance(payload, list) and all(isinstance(page, dict) for page in payload):
        pages = payload
    else:
        raise SystemExit("expected paginated GitHub API objects")
    merged = []
    for page in pages:
        collection = page.get(collection_key)
        if not isinstance(collection, list):
            raise SystemExit(f"GitHub API page lacks {collection_key} list")
        merged.extend(collection)
    return {collection_key: merged}


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
    canonical_tasks = [(True, "1.1 First"), (False, "1.2 Second")]
    assert first_task_mismatch(canonical_tasks, canonical_tasks) is None
    assert first_task_mismatch(
        canonical_tasks, [(True, "1.1 First"), (False, "1.1 First")]
    ) == (1, (False, "1.2 Second"), (False, "1.1 First"))
    assert first_task_mismatch(
        canonical_tasks, [*canonical_tasks, (False, "1.3 Extra")]
    ) == (2, None, (False, "1.3 Extra"))
    assert first_task_mismatch(canonical_tasks, list(reversed(canonical_tasks))) == (
        0,
        (True, "1.1 First"),
        (False, "1.2 Second"),
    )
    assert repo_parts("owner/repo") == ["owner", "repo"]
    assert flatten_paginated_response([[{"number": 1}], [{"number": 2}]]) == [
        {"number": 1},
        {"number": 2},
    ]
    assert flatten_paginated_response([{"number": 3}]) == [{"number": 3}]
    assert merge_paginated_objects(
        [{"jobs": [{"id": 1}]}, {"jobs": [{"id": 2}]}], "jobs"
    ) == {"jobs": [{"id": 1}, {"id": 2}]}
    final_changes = verification_plan_changes(
        {
            "task_sources": [
                {"change": "advance-rust-repository-intelligence"},
                {"change": "enforce-rust-test-quality-gates"},
            ]
        },
        "self-test plan",
    )
    assert final_changes == {
        "advance-rust-repository-intelligence",
        "enforce-rust-test-quality-gates",
    }
    assert "historical-change" not in final_changes
    try:
        verification_plan_changes(
            {"task_sources": [{"change": "advance-rust-repository-intelligence"}]},
            "weakened self-test plan",
        )
        raise AssertionError("weakened final verification scope was accepted")
    except SystemExit:
        pass
    assert expected_commit_is_checkout("a" * 40, "A" * 40)
    assert not expected_commit_is_checkout("a" * 40, "b" * 40)
    assert clean("a\r\n  b") == "a b"
    assert not has_unrendered_newline_escapes("first line\n\nsecond line")
    assert not has_unrendered_newline_escapes(r"Use `\n\n` when testing escapes.")
    assert not has_unrendered_newline_escapes("```text\nfirst\\n\\nsecond\n```")
    assert has_unrendered_newline_escapes(r"first line\nsecond line")
    assert has_unrendered_newline_escapes(r"first line\n\nsecond line\n- item")
    malformed_issue = {
        "body": r"Summary\n\nDetails",
        "comments": [
            {
                "authorAssociation": "OWNER",
                "body": r"Evidence\n\n- check",
                "url": "https://example.invalid/trusted",
            },
            {
                "authorAssociation": "NONE",
                "body": r"External\n\ntext cannot block release",
                "url": "https://example.invalid/untrusted",
            },
        ],
    }
    assert issue_formatting_failures(malformed_issue, 308) == [
        "#308 body contains unrendered newline escapes",
        "#308 trusted comment https://example.invalid/trusted contains unrendered newline escapes",
    ]
    linked_issue = {
        "body": """
## OpenSpec Task Checklist

- [x] 2.1 Implemented task [UT:ARRI-2.1]
  - Unit test: [UT:ARRI-2.1](https://github.com/owner/repo/search?q=test&type=code)
  - GitHub run: [01-CI](https://github.com/owner/repo/actions/runs/123)

- [x] 1.1 Specification task [UT:ARRI-1.1]
"""
    }
    expected = [
        (True, "2.1 Implemented task [UT:ARRI-2.1]"),
        (True, "1.1 Specification task [UT:ARRI-1.1]"),
    ]
    requirements = {
        ("change", "2.1"): {"test_id": "UT:ARRI-2.1", "function": "tests::implemented"}
    }
    exemptions = {("change", "1.1"): "Planning-only specification task."}
    assert test_link_policy(
        {
            "unit_test_exemptions": [
                {
                    "change": "change",
                    "task_id": "1.1",
                    "scope": UNIT_TEST_EXEMPTION_SCOPE,
                    "reason": exemptions[("change", "1.1")],
                }
            ]
        },
        "self-test plan",
    ) == ({}, exemptions)
    assert unknown_test_policy_identities(
        "change",
        expected,
        {("change", "2.1"): requirements[("change", "2.1")]},
        {("change", "9.9"): "unknown task"},
    ) == [("change", "9.9")]
    for invalid_exemption in (
        {"change": "change", "task_id": "1.1", "reason": "missing scope"},
        {
            "change": "change",
            "task_id": "1.1",
            "scope": "all-evidence",
            "reason": "overbroad exemption",
        },
    ):
        try:
            test_link_policy(
                {"unit_test_exemptions": [invalid_exemption]}, "invalid self-test plan"
            )
            raise AssertionError("invalid unit-test exemption was accepted")
        except SystemExit:
            pass
    assert (
        issue_evidence_link_failures(
            linked_issue,
            1,
            "change",
            expected,
            requirements,
            exemptions,
            "owner/repo",
            True,
            False,
            "",
            {},
            {},
        )
        == []
    )
    missing_unit = {"body": linked_issue["body"].replace("[UT:ARRI-2.1](", "[test](")}
    assert issue_evidence_link_failures(
        missing_unit,
        1,
        "change",
        expected,
        requirements,
        exemptions,
        "owner/repo",
        True,
        False,
        "",
        {},
        {},
    ) == ["#1 checked task 2.1 lacks clickable test link UT:ARRI-2.1"]
    missing_run = {
        "body": linked_issue["body"]
        .replace("  - GitHub run:", "  - Hosted evidence:")
        .replace("/actions/runs/123", "/issues/1")
    }
    assert issue_evidence_link_failures(
        missing_run,
        1,
        "change",
        expected,
        requirements,
        exemptions,
        "owner/repo",
        True,
        True,
        "a" * 40,
        {},
        {},
    ) == [
        "#1 checked task 2.1 lacks an exact-final-SHA test permalink for UT:ARRI-2.1",
        "#1 checked task 2.1 lacks final GitHub run link",
        "#1 checked task 1.1 lacks final GitHub run link",
    ]
    assert issue_evidence_link_failures(
        linked_issue,
        1,
        "change",
        expected,
        requirements,
        {},
        "owner/repo",
        True,
        False,
        "",
        {},
        {},
    ) == [
        "#1 checked task 1.1 lacks an implemented test definition or explicit unit-test exemption"
    ]
    final_commit = "a" * 40
    artifact_digest = "b" * 64
    covered_input_digest = "c" * 64
    source_path = "tests/issueops_test.py"
    final_requirement = {
        "test_id": "UT:ARRI-2.1",
        "function": "tests::test_task",
        "command": {
            "executable": "python",
            "arguments": ["-m", "unittest", "tests.test_task"],
        },
        "assertion": "The task-specific behavior passes.",
        "source_path": source_path,
        "source_anchor": "test_task",
        "result": {
            "implementation_commit": final_commit,
            "covered_input_digest": covered_input_digest,
            "run_identity": "github-actions:123:1:task-evidence",
            "artifact_digest": artifact_digest,
            "hosted_evidence": {
                "repository": "owner/repo",
                "test_id": "UT:ARRI-2.1",
                "run_id": 123,
                "run_attempt": 1,
                **FINAL_EVIDENCE_POLICY,
                "job_id": 789,
                "artifact_digest": artifact_digest,
                "commit_sha": final_commit,
            },
        },
    }
    run_payload = {
        "status": "completed",
        "conclusion": "success",
        "head_sha": final_commit,
        "run_attempt": 1,
        "name": FINAL_EVIDENCE_POLICY["workflow_name"],
        "path": FINAL_EVIDENCE_POLICY["workflow_path"],
    }
    jobs_payload = {
        "jobs": [
            {
                "id": 789,
                "name": FINAL_EVIDENCE_POLICY["job_name"],
                "status": "completed",
                "conclusion": "success",
                "run_id": 123,
                "run_attempt": 1,
                "head_sha": final_commit,
                "steps": [
                    {
                        "name": FINAL_EVIDENCE_POLICY["step_name"],
                        "status": "completed",
                        "conclusion": "success",
                    }
                ],
            }
        ]
    }
    artifacts_payload = {
        "artifacts": [
            {
                "id": 456,
                "name": FINAL_EVIDENCE_POLICY["artifact_name"],
                "digest": f"sha256:{artifact_digest}",
                "expired": False,
                "workflow_run": {"id": 123, "head_sha": final_commit},
            }
        ]
    }
    task_evidence_manifest = {
        "format": TASK_EVIDENCE_FORMAT,
        "repository": "owner/repo",
        "commit_sha": final_commit,
        "run_id": 123,
        "run_attempt": 1,
        "tests": [
            {
                "test_id": "UT:ARRI-2.1",
                "state": "passed",
                "command_digest": canonical_sha256(final_requirement["command"]),
                "assertion_digest": canonical_sha256(final_requirement["assertion"]),
                "covered_input_digest": covered_input_digest,
                "implementation_commit": final_commit,
            }
        ],
    }
    run_cache = {
        123: {
            "run": run_payload,
            "jobs": jobs_payload,
            "artifacts": artifacts_payload,
            "artifact_manifests": {456: task_evidence_manifest},
        }
    }
    source_cache = {
        (final_commit, source_path): "def helper():\n    pass\n\ndef test_task():\n    pass\n"
    }
    final_issue = {
        "body": f"""
## OpenSpec Task Checklist

- [x] 2.1 Implemented task [UT:ARRI-2.1]
  - Unit test: [UT:ARRI-2.1](https://github.com/owner/repo/blob/{final_commit}/{source_path}#L4)
  - GitHub run: [task evidence](https://github.com/owner/repo/actions/runs/123)
"""
    }
    final_expected = [(True, "2.1 Implemented task [UT:ARRI-2.1]")]
    final_requirements = {("change", "2.1"): final_requirement}
    assert implemented_test_contract_is_valid(
        {
            "test_id": final_requirement["test_id"],
            "function": final_requirement["function"],
            "command": final_requirement["command"],
            "assertion": final_requirement["assertion"],
        }
    )
    assert not implemented_test_contract_is_valid(
        {
            "test_id": final_requirement["test_id"],
            "function": final_requirement["function"],
            "command": None,
            "assertion": None,
        }
    )
    assert exact_test_link_is_valid(
        "owner/repo",
        f"https://github.com/owner/repo/blob/{final_commit}/{source_path}#L4",
        final_commit,
        final_requirement,
        source_cache,
    )
    assert not exact_test_link_is_valid(
        "owner/repo",
        f"https://github.com/owner/repo/blob/{final_commit}/{source_path}#L3-L4",
        final_commit,
        final_requirement,
        source_cache,
    )
    comment_source_cache = {
        (final_commit, source_path): "# def test_task():\ndef other():\n    pass\n"
    }
    assert not exact_test_link_is_valid(
        "owner/repo",
        f"https://github.com/owner/repo/blob/{final_commit}/{source_path}#L1",
        final_commit,
        final_requirement,
        comment_source_cache,
    )
    rust_requirement = {
        **final_requirement,
        "source_path": "tests/issueops_test.rs",
    }
    for invalid_rust_source, invalid_line in (
        ('const X: &str = "fn test_task()";\n', 1),
        ("/*\nfn test_task() {}\n*/\n", 2),
        ("/* outer\n/* nested */\nfn test_task() {}\n*/\n", 3),
        ('const X: &str = r#"\nfn test_task() {}\n"#;\n', 2),
    ):
        assert not source_definition_is_at_line(
            invalid_rust_source,
            rust_requirement["source_path"],
            "test_task",
            invalid_line,
        )
    assert issue_evidence_link_failures(
        final_issue,
        1,
        "change",
        final_expected,
        final_requirements,
        {},
        "owner/repo",
        True,
        True,
        final_commit,
        run_cache,
        source_cache,
    ) == []
    exempt_final_issue = {
        "body": """
## OpenSpec Task Checklist

- [x] 1.1 Specification task [UT:ARRI-1.1]
  - GitHub run: [task evidence](https://github.com/owner/repo/actions/runs/123)
"""
    }
    assert issue_evidence_link_failures(
        exempt_final_issue,
        1,
        "change",
        [(True, "1.1 Specification task [UT:ARRI-1.1]")],
        {},
        exemptions,
        "owner/repo",
        True,
        True,
        final_commit,
        run_cache,
        {},
    ) == []
    unrelated_exempt_run_cache = {123: json.loads(json.dumps(run_cache[123]))}
    unrelated_exempt_run_cache[123]["run"]["path"] = ".github/workflows/unrelated.yml"
    assert issue_evidence_link_failures(
        exempt_final_issue,
        1,
        "change",
        [(True, "1.1 Specification task [UT:ARRI-1.1]")],
        {},
        exemptions,
        "owner/repo",
        True,
        True,
        final_commit,
        unrelated_exempt_run_cache,
        {},
    ) == [f"#1 checked task 1.1 has no successful GitHub run for {final_commit}"]
    for invalid_url in (
        "https://github.com/owner/repo/issues/1",
        f"https://github.com/owner/repo/blob/{'d' * 40}/{source_path}#L4",
        f"https://github.com/owner/repo/blob/{final_commit}/tests/wrong.py#L4",
        f"https://github.com/owner/repo/blob/{final_commit}/{source_path}#L0",
        f"https://github.com/owner/repo/blob/{final_commit}/{source_path}#L3",
        f"https://github.com/owner/repo/blob/{final_commit}/{source_path}#L99",
    ):
        invalid_issue = {
            "body": re.sub(
                rf"https://github\.com/owner/repo/blob/{final_commit}/[^)]+",
                invalid_url,
                final_issue["body"],
            )
        }
        failures = issue_evidence_link_failures(
            invalid_issue,
            1,
            "change",
            final_expected,
            final_requirements,
            {},
            "owner/repo",
            True,
            True,
            final_commit,
            run_cache,
            source_cache,
        )
        assert failures == [
            "#1 checked task 2.1 lacks an exact-final-SHA test permalink for UT:ARRI-2.1"
        ]
    unchecked_issue = {
        "body": "## OpenSpec Task Checklist\n\n- [ ] 2.1 Pending task [UT:ARRI-2.1]\n"
    }
    assert issue_evidence_link_failures(
        unchecked_issue,
        1,
        "change",
        [(False, "2.1 Pending task [UT:ARRI-2.1]")],
        {},
        {},
        "owner/repo",
        True,
        True,
        final_commit,
        {},
        {},
    ) == ["#1 task 2.1 is incomplete at final closeout"]
    assert hosted_evidence_payload_is_successful(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        jobs_payload,
        artifacts_payload,
        {456: task_evidence_manifest},
    )
    spoofed_requirement = json.loads(json.dumps(final_requirement))
    spoofed_hosted = spoofed_requirement["result"]["hosted_evidence"]
    spoofed_hosted.update(
        {
            "workflow_name": "Documentation",
            "workflow_path": ".github/workflows/docs.yml",
            "job_name": "publish-docs",
            "step_name": "Pretend task verification",
            "artifact_name": "documentation-output",
        }
    )
    spoofed_run = {
        **run_payload,
        "name": "Documentation",
        "path": ".github/workflows/docs.yml",
    }
    spoofed_jobs = json.loads(json.dumps(jobs_payload))
    spoofed_jobs["jobs"][0]["name"] = "publish-docs"
    spoofed_jobs["jobs"][0]["steps"][0]["name"] = "Pretend task verification"
    spoofed_artifacts = json.loads(json.dumps(artifacts_payload))
    spoofed_artifacts["artifacts"][0]["name"] = "documentation-output"
    assert not hosted_evidence_payload_is_successful(
        spoofed_requirement,
        "owner/repo",
        123,
        final_commit,
        spoofed_run,
        spoofed_jobs,
        spoofed_artifacts,
        {456: task_evidence_manifest},
    )
    unrelated_run = {**run_payload, "path": ".github/workflows/unrelated.yml"}
    assert not hosted_evidence_payload_is_successful(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        unrelated_run,
        jobs_payload,
        artifacts_payload,
        {456: task_evidence_manifest},
    )
    wrong_jobs = {
        "jobs": [{**jobs_payload["jobs"][0], "name": "unrelated-green-job"}]
    }
    assert not hosted_evidence_payload_is_successful(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        wrong_jobs,
        artifacts_payload,
        {456: task_evidence_manifest},
    )
    duplicate_jobs = {"jobs": [jobs_payload["jobs"][0], jobs_payload["jobs"][0]]}
    assert not hosted_evidence_payload_is_successful(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        duplicate_jobs,
        artifacts_payload,
        {456: task_evidence_manifest},
    )
    failed_step_jobs = json.loads(json.dumps(jobs_payload))
    failed_step_jobs["jobs"][0]["steps"][0]["conclusion"] = "failure"
    assert not hosted_evidence_payload_is_successful(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        failed_step_jobs,
        artifacts_payload,
        {456: task_evidence_manifest},
    )
    wrong_artifacts = {
        "artifacts": [
            {**artifacts_payload["artifacts"][0], "name": "unrelated-artifact"}
        ]
    }
    assert not hosted_evidence_payload_is_successful(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        jobs_payload,
        wrong_artifacts,
        {456: task_evidence_manifest},
    )
    wrong_test_requirement = json.loads(json.dumps(final_requirement))
    wrong_test_requirement["result"]["hosted_evidence"]["test_id"] = "UT:OTHER"
    assert not hosted_evidence_payload_is_successful(
        wrong_test_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        jobs_payload,
        artifacts_payload,
        {456: task_evidence_manifest},
    )
    unrelated_manifest = json.loads(json.dumps(task_evidence_manifest))
    unrelated_manifest["tests"][0]["test_id"] = "UT:OTHER"
    assert not hosted_evidence_payload_is_successful(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        jobs_payload,
        artifacts_payload,
        {456: unrelated_manifest},
    )
    conflicting_manifest = json.loads(json.dumps(task_evidence_manifest))
    conflicting_row = json.loads(json.dumps(conflicting_manifest["tests"][0]))
    conflicting_row["state"] = "failed"
    conflicting_manifest["tests"].append(conflicting_row)
    assert not task_evidence_manifest_is_valid(
        final_requirement,
        "owner/repo",
        123,
        final_commit,
        1,
        conflicting_manifest,
    )
    synthetic_digest_requirement = json.loads(json.dumps(final_requirement))
    synthetic_digest_requirement["result"]["artifact_digest"] = "b" * 40
    synthetic_digest_requirement["result"]["hosted_evidence"][
        "artifact_digest"
    ] = "b" * 40
    assert not hosted_evidence_payload_is_successful(
        synthetic_digest_requirement,
        "owner/repo",
        123,
        final_commit,
        run_payload,
        jobs_payload,
        artifacts_payload,
        {456: task_evidence_manifest},
    )
    assert github_run_id(
        "https://github.com/owner/repo/actions/runs/123/job/456", "owner/repo"
    ) == 123
    assert github_run_id(
        "https://github.com/attacker/repo/actions/runs/123", "owner/repo"
    ) is None
    assert run_payload_is_successful(
        {"status": "completed", "conclusion": "success", "head_sha": "a" * 40},
        "a" * 40,
    )
    assert not run_payload_is_successful(
        {"status": "completed", "conclusion": "failure", "head_sha": "a" * 40},
        "a" * 40,
    )
    skipped_final = subprocess.run(
        [
            sys.executable,
            str(Path(__file__)),
            "--repo",
            "owner/repo",
            "--skip-openspec",
            "--require-run-links",
            "--expected-commit",
            "a" * 40,
        ],
        capture_output=True,
        text=True,
        timeout=COMMAND_TIMEOUT_SECONDS,
    )
    assert skipped_final.returncode != 0
    assert "--skip-openspec is forbidden" in skipped_final.stderr
    print("issue checklist self-test passed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="")
    parser.add_argument("--root", default=".")
    parser.add_argument("--issue-map", default="openspec/issue-map.json")
    parser.add_argument("--verification-plan", default="openspec/task-verification-plan.json")
    parser.add_argument("--milestone", action="append", default=[])
    parser.add_argument("--skip-openspec", action="store_true")
    parser.add_argument("--require-run-links", action="store_true")
    parser.add_argument("--expected-commit", default="")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return

    if not args.repo:
        raise SystemExit("--repo is required unless --self-test is used")
    if args.require_run_links and args.skip_openspec:
        raise SystemExit("--skip-openspec is forbidden with --require-run-links")
    if args.require_run_links and not COMMIT_RE.fullmatch(args.expected_commit):
        raise SystemExit("--require-run-links requires a full 40-character --expected-commit")

    root = Path(args.root)
    if args.require_run_links and not expected_commit_is_checkout(
        args.expected_commit, repository_head(root)
    ):
        raise SystemExit(
            "--expected-commit must equal the checked-out git HEAD in final closeout mode"
        )
    failures = []
    if not args.skip_openspec:
        test_requirements, test_exemptions = load_test_link_policy(
            args.verification_plan
        )
        failures.extend(
            check_openspec_tasks(
                args.repo,
                root,
                load_issue_map(args.issue_map),
                test_requirements,
                test_exemptions,
                args.require_run_links,
                args.expected_commit,
                load_verification_plan_changes(args.verification_plan),
            )
        )
    for milestone in args.milestone:
        failures.extend(check_milestone_complete(args.repo, milestone))

    if failures:
        print("\nIssue checklist validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
