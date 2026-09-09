#!/usr/bin/env python3
"""Bounded repository-local improvement loop driven by coding-tooling evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
from typing import Any, Sequence

DEFAULT_MAX_CANDIDATES = 5
DEFAULT_MAX_REPAIRS = 3
MAX_CANDIDATES = 10
MAX_REPAIRS = 5
PROTECTED_PATHS = (
    ".coding-tooling.json",
    ".github/workflows/ci.yml",
    "scripts/test-integration.sh",
)


class LoopError(RuntimeError):
    pass


def run(command: Sequence[str], *, cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(command),
        cwd=cwd,
        check=False,
        text=True,
        capture_output=True,
    )


def repository_root() -> Path:
    result = run(["git", "rev-parse", "--show-toplevel"], cwd=Path.cwd())
    if result.returncode != 0:
        raise LoopError(result.stderr.strip() or "not inside a Git repository")
    return Path(result.stdout.strip()).resolve()


def resolve_coding_tooling(root: Path) -> list[str]:
    override = os.environ.get("CODING_TOOLING_COMMAND")
    if override:
        command = shlex.split(override)
        if not command:
            raise LoopError("CODING_TOOLING_COMMAND is empty")
        return command

    executable = shutil.which("coding-tooling")
    if executable:
        return [executable]

    tooling_dir = Path(os.environ.get("CODING_TOOLING_DIR", root.parent / "coding-tooling"))
    cli = tooling_dir / "src" / "cli.ts"
    bun = shutil.which("bun")
    if bun and cli.is_file():
        return [bun, str(cli)]

    raise LoopError(
        "coding-tooling is required; install it, set CODING_TOOLING_COMMAND, "
        "or point CODING_TOOLING_DIR at a checkout"
    )


def resolve_agent_command(explicit: str | None) -> list[str] | None:
    raw = explicit or os.environ.get("CODING_TOOLING_LOOP_AGENT_COMMAND")
    if raw:
        command = shlex.split(raw)
        if not command:
            raise LoopError("agent command is empty")
        return command

    codex = shutil.which("codex")
    if codex:
        return [codex, "exec", "--json", "--approve-for-me", "{prompt}"]
    return None


def write_log(path: Path, result: subprocess.CompletedProcess[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        f"returncode={result.returncode}\n\nSTDOUT\n{result.stdout}\n\nSTDERR\n{result.stderr}\n",
        encoding="utf-8",
    )


def parse_passed_json(stdout: str, *, operation: str) -> dict[str, Any]:
    try:
        payload = json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise LoopError(f"{operation} returned invalid JSON: {exc}") from exc
    if not isinstance(payload, dict):
        raise LoopError(f"{operation} returned a non-object result")
    if payload.get("status") != "passed":
        raise LoopError(f"{operation} did not pass: {payload.get('diagnostics', [])}")
    return payload


def remediation_candidates(payload: dict[str, Any]) -> list[dict[str, Any]]:
    data = payload.get("data")
    if not isinstance(data, dict) or not isinstance(data.get("candidates"), list):
        raise LoopError("remediation plan is missing candidates")
    candidates = data["candidates"]
    if not all(isinstance(candidate, dict) for candidate in candidates):
        raise LoopError("remediation plan contains a malformed candidate")
    return candidates


def substitute_tooling(command: Sequence[str], tooling: Sequence[str]) -> list[str]:
    if command and command[0] == "coding-tooling":
        return [*tooling, *command[1:]]
    return list(command)


def render_agent_command(command: Sequence[str], prompt: str) -> list[str]:
    rendered: list[str] = []
    replaced = False
    for token in command:
        if "{prompt}" in token:
            rendered.append(token.replace("{prompt}", prompt))
            replaced = True
        else:
            rendered.append(token)
    if not replaced:
        rendered.append(prompt)
    return rendered


def git_identity(root: Path) -> tuple[str, str]:
    branch = run(["git", "branch", "--show-current"], cwd=root)
    head = run(["git", "rev-parse", "HEAD"], cwd=root)
    if branch.returncode != 0 or head.returncode != 0:
        raise LoopError("unable to read Git identity")
    return branch.stdout.strip(), head.stdout.strip()


def require_clean_start(root: Path) -> None:
    result = run(["git", "status", "--porcelain=v1", "--untracked-files=all"], cwd=root)
    if result.returncode != 0:
        raise LoopError("unable to inspect the working tree")
    if result.stdout.strip():
        raise LoopError("working tree must be clean before the improvement loop starts")


def worktree_fingerprint(root: Path) -> str:
    diff = run(["git", "diff", "--binary", "HEAD"], cwd=root)
    status = run(["git", "status", "--porcelain=v1", "--untracked-files=all"], cwd=root)
    if diff.returncode != 0 or status.returncode != 0:
        raise LoopError("unable to fingerprint the working tree")
    digest = hashlib.sha256()
    digest.update(diff.stdout.encode())
    digest.update(b"\0")
    digest.update(status.stdout.encode())
    return digest.hexdigest()


def control_hashes(root: Path) -> dict[str, str | None]:
    return {
        path: hashlib.sha256((root / path).read_bytes()).hexdigest()
        if (root / path).is_file()
        else None
        for path in PROTECTED_PATHS
    }


def candidate_owned_paths(candidate: dict[str, Any]) -> set[str]:
    paths: set[str] = set()
    related = candidate.get("relatedFiles")
    if isinstance(related, list):
        paths.update(item for item in related if isinstance(item, str))
    scaffolds = candidate.get("scaffolds")
    if isinstance(scaffolds, list):
        for scaffold in scaffolds:
            if isinstance(scaffold, dict) and isinstance(scaffold.get("path"), str):
                paths.add(scaffold["path"])
    return paths


def changed_protected_paths(
    before: dict[str, str | None], after: dict[str, str | None], candidate: dict[str, Any]
) -> list[str]:
    allowed = candidate_owned_paths(candidate)
    return sorted(
        path
        for path in PROTECTED_PATHS
        if before.get(path) != after.get(path) and path not in allowed
    )


def remediation_plan(
    root: Path,
    tooling: Sequence[str],
    *,
    include_baseline: bool,
    artifact_dir: Path,
) -> list[dict[str, Any]]:
    command = [*tooling, "remediation", "plan"]
    if include_baseline:
        command.append("--include-baseline")
    command.append("--json")
    result = run(command, cwd=root)
    log = artifact_dir / "remediation-plan.log"
    write_log(log, result)
    if result.returncode != 0:
        raise LoopError(f"coding-tooling remediation plan failed; see {log}")
    return remediation_candidates(
        parse_passed_json(result.stdout, operation="coding-tooling remediation plan")
    )


def verification_commands(candidate: dict[str, Any]) -> list[list[str]]:
    raw = candidate.get("verification", [])
    if not isinstance(raw, list):
        raise LoopError("candidate verification is malformed")
    commands: list[list[str]] = []
    for command in raw:
        if not isinstance(command, list) or not all(isinstance(part, str) for part in command):
            raise LoopError("candidate verification command is malformed")
        commands.append(command)
    return commands


def apply_scaffolds(
    root: Path,
    tooling: Sequence[str],
    candidate: dict[str, Any],
    *,
    artifact_dir: Path,
) -> tuple[bool, Path | None]:
    raw = candidate.get("scaffolds", [])
    if not isinstance(raw, list):
        raise LoopError("candidate scaffolds are malformed")
    scaffolds = [item for item in raw if isinstance(item, dict)]
    if len(scaffolds) != len(raw):
        raise LoopError("candidate contains a malformed scaffold")
    if not scaffolds:
        return False, None

    for index, scaffold in enumerate(scaffolds, start=1):
        command = scaffold.get("command")
        if not isinstance(command, list) or not all(isinstance(part, str) for part in command):
            raise LoopError("candidate scaffold command is malformed")
        result = run(substitute_tooling(command, tooling), cwd=root)
        log = artifact_dir / f"scaffold-{index}.log"
        write_log(log, result)
        if result.returncode != 0:
            return True, log
    return True, None


def build_prompt(candidate: dict[str, Any], failure_logs: Sequence[Path]) -> str:
    previous = ""
    if failure_logs:
        previous = f"\nPrevious verification failed; inspect {failure_logs[0]} before editing.\n"
    return f"""Resolve exactly this coding-tooling remediation candidate in social-service.

Read AGENTS.md before editing. Keep the modular-monolith boundary, app_id scoping, visibility rules,
feature-gate semantics, group authority, and moderation boundaries intact. Prefer the smallest vertical fix.
Do not baseline or suppress findings. Do not weaken or bypass tests, coding-tooling, or CI.
Do not commit, push, merge, switch branches, publish, tag, or change repository governance.
Do not edit {', '.join(PROTECTED_PATHS)} unless the candidate itself names that path as evidence.
Run the narrowest meaningful verification after the edit; the outer loop independently re-verifies it.
{previous}
Candidate:
{json.dumps(candidate, indent=2, sort_keys=True)}
"""


def invoke_agent(
    root: Path,
    command: Sequence[str],
    candidate: dict[str, Any],
    *,
    failure_logs: Sequence[Path],
    artifact_dir: Path,
    attempt: int,
) -> Path | None:
    result = run(render_agent_command(command, build_prompt(candidate, failure_logs)), cwd=root)
    log = artifact_dir / f"agent-attempt-{attempt}.log"
    write_log(log, result)
    return log if result.returncode != 0 else None


def run_candidate_verification(
    root: Path,
    tooling: Sequence[str],
    candidate: dict[str, Any],
    *,
    artifact_dir: Path,
) -> list[Path]:
    for index, command in enumerate(verification_commands(candidate), start=1):
        result = run(substitute_tooling(command, tooling), cwd=root)
        log = artifact_dir / f"verification-{index}.log"
        write_log(log, result)
        if result.returncode != 0:
            return [log]
    return []


def run_tier(
    root: Path,
    tooling: Sequence[str],
    tier: str,
    *,
    artifact_dir: Path,
    label: str,
) -> list[Path]:
    report = artifact_dir / f"{label}-{tier}.json"
    result = run(
        [
            *tooling,
            "run",
            "--tier",
            tier,
            "--strict",
            "--report",
            str(report),
            "--json",
        ],
        cwd=root,
    )
    log = artifact_dir / f"{label}-{tier}.log"
    write_log(log, result)
    return [] if result.returncode == 0 else [log]


def repair_candidate(
    root: Path,
    tooling: Sequence[str],
    agent_command: Sequence[str] | None,
    candidate: dict[str, Any],
    *,
    artifact_dir: Path,
    max_repairs: int,
) -> None:
    candidate_id = str(candidate.get("id") or "candidate")
    if candidate.get("kind") == "review":
        raise LoopError(f"{candidate_id} requires explicit review; automatic mutation is not authorized")

    candidate_dir = artifact_dir / candidate_id
    candidate_dir.mkdir(parents=True, exist_ok=True)
    failure_logs: list[Path] = []

    for attempt in range(1, max_repairs + 1):
        identity_before = git_identity(root)
        fingerprint_before = worktree_fingerprint(root)
        controls_before = control_hashes(root)

        scaffolded, mutation_failure = apply_scaffolds(
            root, tooling, candidate, artifact_dir=candidate_dir
        )
        if not scaffolded:
            if candidate.get("kind") == "deterministic-scaffold":
                raise LoopError(f"{candidate_id} declares deterministic scaffolding but none is available")
            if agent_command is None:
                raise LoopError(f"{candidate_id} requires an agent but no agent command is available")
            mutation_failure = invoke_agent(
                root,
                agent_command,
                candidate,
                failure_logs=failure_logs,
                artifact_dir=candidate_dir,
                attempt=attempt,
            )

        if git_identity(root) != identity_before:
            raise LoopError(f"{candidate_id} moved the branch or HEAD")
        unauthorized = changed_protected_paths(
            controls_before, control_hashes(root), candidate
        )
        if unauthorized:
            raise LoopError(
                f"{candidate_id} changed protected controls without candidate evidence: "
                + ", ".join(unauthorized)
            )
        if mutation_failure is not None:
            raise LoopError(f"mutation failed; see {mutation_failure}")
        if worktree_fingerprint(root) == fingerprint_before:
            raise LoopError(f"{candidate_id} made no repository progress")

        failure_logs = run_candidate_verification(
            root, tooling, candidate, artifact_dir=candidate_dir
        )
        if failure_logs:
            continue
        failure_logs = run_tier(
            root,
            tooling,
            "fast",
            artifact_dir=candidate_dir,
            label=f"attempt-{attempt}",
        )
        if not failure_logs:
            return

    raise LoopError(
        f"{candidate_id} did not converge after {max_repairs} repair attempts; see {candidate_dir}"
    )


def bounded_count(value: int, *, name: str, maximum: int) -> int:
    if not 1 <= value <= maximum:
        raise argparse.ArgumentTypeError(f"{name} must be between 1 and {maximum}")
    return value


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Run a bounded coding-tooling improvement loop")
    parser.add_argument("--agent-command")
    parser.add_argument("--include-baseline", action="store_true")
    parser.add_argument(
        "--max-candidates",
        type=lambda value: bounded_count(int(value), name="max-candidates", maximum=MAX_CANDIDATES),
        default=DEFAULT_MAX_CANDIDATES,
    )
    parser.add_argument(
        "--max-repairs",
        type=lambda value: bounded_count(int(value), name="max-repairs", maximum=MAX_REPAIRS),
        default=DEFAULT_MAX_REPAIRS,
    )
    args = parser.parse_args(argv)

    root = repository_root()
    require_clean_start(root)
    artifact_dir = root / ".artifacts" / "coding-tooling" / "loop"
    artifact_dir.mkdir(parents=True, exist_ok=True)
    tooling = resolve_coding_tooling(root)
    agent_command = resolve_agent_command(args.agent_command)

    for index in range(1, args.max_candidates + 1):
        candidates = remediation_plan(
            root,
            tooling,
            include_baseline=args.include_baseline,
            artifact_dir=artifact_dir,
        )
        if not candidates:
            failures = run_tier(
                root, tooling, "full", artifact_dir=artifact_dir, label="final"
            )
            if failures:
                raise LoopError(f"final full tier failed; see {failures[0]}")
            print("coding-tooling improvement loop: converged and full tier passed")
            return 0

        candidate = candidates[0]
        print(
            f"coding-tooling improvement loop: candidate {index}/{args.max_candidates} "
            f"{candidate.get('id', '<unknown>')}: {candidate.get('summary', '')}"
        )
        repair_candidate(
            root,
            tooling,
            agent_command,
            candidate,
            artifact_dir=artifact_dir,
            max_repairs=args.max_repairs,
        )

    remaining = remediation_plan(
        root,
        tooling,
        include_baseline=args.include_baseline,
        artifact_dir=artifact_dir,
    )
    if remaining:
        raise LoopError(
            f"reached --max-candidates with {len(remaining)} remediation candidate(s) still active"
        )

    failures = run_tier(root, tooling, "full", artifact_dir=artifact_dir, label="final")
    if failures:
        raise LoopError(f"final full tier failed; see {failures[0]}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except LoopError as exc:
        print(f"coding-tooling improvement loop: {exc}", file=sys.stderr)
        raise SystemExit(1)
