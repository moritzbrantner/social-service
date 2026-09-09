import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import coding_tooling_loop as loop  # noqa: E402


class CodingToolingLoopTests(unittest.TestCase):
    def test_parse_passed_json_fails_closed(self):
        payload = loop.parse_passed_json(
            json.dumps({"status": "passed", "data": {"candidates": []}}),
            operation="plan",
        )
        self.assertEqual(payload["status"], "passed")

        for value in (
            "not json",
            json.dumps([]),
            json.dumps({"status": "unavailable", "diagnostics": []}),
        ):
            with self.assertRaises(loop.LoopError):
                loop.parse_passed_json(value, operation="plan")

    def test_remediation_candidates_require_object_list(self):
        self.assertEqual(
            loop.remediation_candidates({"data": {"candidates": [{"id": "CT-1"}]}}),
            [{"id": "CT-1"}],
        )
        for payload in (
            {},
            {"data": {}},
            {"data": {"candidates": "wrong"}},
            {"data": {"candidates": ["wrong"]}},
        ):
            with self.assertRaises(loop.LoopError):
                loop.remediation_candidates(payload)

    def test_tooling_commands_use_the_resolved_command(self):
        self.assertEqual(
            loop.substitute_tooling(
                ["coding-tooling", "finding", "CT-1", "--json"],
                ["bun", "/tmp/coding-tooling/src/cli.ts"],
            ),
            ["bun", "/tmp/coding-tooling/src/cli.ts", "finding", "CT-1", "--json"],
        )
        self.assertEqual(
            loop.substitute_tooling(["cargo", "test"], ["coding-tooling"]),
            ["cargo", "test"],
        )

    def test_agent_prompt_is_passed_without_shell_interpolation(self):
        self.assertEqual(
            loop.render_agent_command(["codex", "exec", "{prompt}"], "fix\nthis"),
            ["codex", "exec", "fix\nthis"],
        )
        self.assertEqual(
            loop.render_agent_command(["claude", "-p"], "fix this"),
            ["claude", "-p", "fix this"],
        )

    def test_protected_control_changes_require_candidate_evidence(self):
        self.assertIn("scripts/coding_tooling_loop.py", loop.PROTECTED_PATHS)
        before = {path: "before" for path in loop.PROTECTED_PATHS}
        after = dict(before)
        after[".coding-tooling.json"] = "after"

        self.assertEqual(
            loop.changed_protected_paths(before, after, {"relatedFiles": []}),
            [".coding-tooling.json"],
        )
        self.assertEqual(
            loop.changed_protected_paths(
                before,
                after,
                {"relatedFiles": [".coding-tooling.json"]},
            ),
            [],
        )

    def test_worktree_fingerprint_tracks_untracked_content_changes(self):
        successful = lambda command, stdout="": subprocess.CompletedProcess(command, 0, stdout, "")
        responses = [
            successful(["git", "diff"]),
            successful(["git", "status"], "?? candidate.rs\n"),
            successful(["git", "ls-files"], "candidate.rs\0"),
            successful(["git", "diff"]),
            successful(["git", "status"], "?? candidate.rs\n"),
            successful(["git", "ls-files"], "candidate.rs\0"),
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "candidate.rs"
            path.write_text("first", encoding="utf-8")
            with patch.object(loop, "run", side_effect=responses):
                before = loop.worktree_fingerprint(root)
                path.write_text("second", encoding="utf-8")
                after = loop.worktree_fingerprint(root)
        self.assertNotEqual(before, after)

    def test_candidate_verification_stops_on_first_failure(self):
        candidate = {"verification": [["first"], ["second"]]}
        failure = subprocess.CompletedProcess(["first"], 1, "", "failed")
        with tempfile.TemporaryDirectory() as directory, patch.object(
            loop, "run", return_value=failure
        ) as mocked_run:
            failures = loop.run_candidate_verification(
                Path(directory),
                ["coding-tooling"],
                candidate,
                artifact_dir=Path(directory) / "artifacts",
            )

        self.assertEqual(len(failures), 1)
        self.assertEqual(mocked_run.call_count, 1)

    def test_review_candidate_requires_explicit_review(self):
        with self.assertRaisesRegex(loop.LoopError, "explicit review"):
            loop.repair_candidate(
                Path("."),
                ["coding-tooling"],
                ["codex", "exec", "{prompt}"],
                {"id": "CT-REVIEW", "kind": "review"},
                artifact_dir=Path(".artifacts/coding-tooling/loop"),
                max_repairs=1,
            )

    def test_failed_agent_cannot_hide_head_movement(self):
        candidate = {"id": "CT-IMPL", "kind": "implementation", "scaffolds": []}
        failure_log = Path("agent.log")
        with tempfile.TemporaryDirectory() as directory, patch.object(
            loop, "git_identity", side_effect=[("branch", "before"), ("branch", "after")]
        ), patch.object(loop, "worktree_fingerprint", return_value="before"), patch.object(
            loop, "control_hashes", return_value={}
        ), patch.object(loop, "invoke_agent", return_value=failure_log):
            with self.assertRaisesRegex(loop.LoopError, "moved the branch or HEAD"):
                loop.repair_candidate(
                    Path(directory),
                    ["coding-tooling"],
                    ["codex", "exec", "{prompt}"],
                    candidate,
                    artifact_dir=Path(directory) / "artifacts",
                    max_repairs=1,
                )

    def test_deterministic_scaffold_does_not_require_an_agent(self):
        candidate = {
            "id": "CT-SCAFFOLD",
            "kind": "deterministic-scaffold",
            "scaffolds": [{"command": ["coding-tooling", "scaffold", "CT-1"]}],
            "verification": [],
        }
        with tempfile.TemporaryDirectory() as directory, patch.object(
            loop, "git_identity", side_effect=[("branch", "head"), ("branch", "head")]
        ), patch.object(
            loop, "worktree_fingerprint", side_effect=["before", "after"]
        ), patch.object(loop, "control_hashes", return_value={}), patch.object(
            loop, "apply_scaffolds", return_value=(True, None)
        ), patch.object(loop, "run_candidate_verification", return_value=[]), patch.object(
            loop, "run_tier", return_value=[]
        ), patch.object(loop, "invoke_agent") as agent:
            loop.repair_candidate(
                Path(directory),
                ["coding-tooling"],
                None,
                candidate,
                artifact_dir=Path(directory) / "artifacts",
                max_repairs=1,
            )

        agent.assert_not_called()

    def test_partial_scaffold_escalates_to_agent_only_after_first_attempt(self):
        candidate = {
            "id": "CT-PARTIAL",
            "kind": "implementation",
            "scaffolds": [{"command": ["coding-tooling", "scaffold", "CT-1"]}],
            "verification": [],
        }
        first_failure = [Path("verification-1.log")]
        with tempfile.TemporaryDirectory() as directory, patch.object(
            loop,
            "git_identity",
            side_effect=[
                ("branch", "head"),
                ("branch", "head"),
                ("branch", "head"),
                ("branch", "head"),
            ],
        ), patch.object(
            loop, "worktree_fingerprint", side_effect=["before", "scaffolded", "scaffolded", "fixed"]
        ), patch.object(loop, "control_hashes", return_value={}), patch.object(
            loop, "apply_scaffolds", return_value=(True, None)
        ) as scaffolds, patch.object(
            loop, "invoke_agent", return_value=None
        ) as agent, patch.object(
            loop, "run_candidate_verification", side_effect=[first_failure, []]
        ), patch.object(loop, "run_tier", return_value=[]):
            loop.repair_candidate(
                Path(directory),
                ["coding-tooling"],
                ["codex", "exec", "{prompt}"],
                candidate,
                artifact_dir=Path(directory) / "artifacts",
                max_repairs=2,
            )

        self.assertEqual(scaffolds.call_count, 1)
        self.assertEqual(agent.call_count, 1)
        self.assertEqual(agent.call_args.kwargs["attempt"], 2)

    def test_loop_bounds_reject_runaway_values(self):
        self.assertEqual(loop.bounded_count(3, name="repairs", maximum=5), 3)
        for value in (0, 6):
            with self.assertRaises(argparse.ArgumentTypeError):
                loop.bounded_count(value, name="repairs", maximum=5)


if __name__ == "__main__":
    unittest.main()
