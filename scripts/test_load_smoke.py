"""Acceptance through the declared load:smoke command."""
import json
import os
import signal
import shutil
import sys
import time
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

class LoadSmoke(unittest.TestCase):
    def invoke(self, *arguments):
        profiler = ROOT / ".artifacts/tools/runtime-profiler/bin/runtime-profiler"
        self.assertTrue(profiler.is_file(), "install the declared profiler before acceptance")
        result = subprocess.run(["python3", "scripts/load_smoke.py", "--profiler", str(profiler), *arguments], cwd=ROOT, capture_output=True, text=True, timeout=300)
        return result, json.loads(result.stdout)

    def test_real_profile_and_timeline_capture_is_nonempty_and_immutable(self):
        result, report = self.invoke()
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["measuredRequests"], 96)
        bundle = ROOT / report["bundle"]
        original = (bundle / "manifest.json").read_bytes()
        ids = {metric["id"] for metric in report["metrics"]}
        self.assertEqual(ids, {"http.latency", "http.success_rate", "http.error_rate", "http.throughput"})
        repeated, next_report = self.invoke()
        self.assertEqual(repeated.returncode, 0, repeated.stdout)
        self.assertNotEqual(next_report["bundle"], report["bundle"])
        self.assertEqual((bundle / "manifest.json").read_bytes(), original)

    def test_actual_bad_identity_status_fails_after_native_capture(self):
        scenario = json.loads((ROOT / ".performance/load-smoke.json").read_text())
        scenario["target"]["fixture"]["working_directory"] = str(ROOT)
        scenario["target"]["requests"][0]["headers"]["x-app-id"] = "not-a-uuid"
        scenario["target"]["request_count"] = 2
        scenario["target"]["concurrency"] = 1
        scenario["run"]["warmup_iterations"] = 0
        scenario["run"]["measurement_iterations"] = 1
        (ROOT / ".artifacts").mkdir(exist_ok=True)
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", dir=ROOT / ".artifacts") as source:
            json.dump(scenario, source)
            source.flush()
            result, report = self.invoke("--scenario", source.name)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["measuredRequests"], 2)
        evidence = json.loads((ROOT / report["bundle"] / "http-workload.json").read_text())
        self.assertEqual(evidence["samples"][0]["status_code"], 400)
        self.assertFalse(evidence["samples"][0]["succeeded"])

    def test_missing_curl_reports_real_collector_unavailability(self):
        profiler = ROOT / ".artifacts/tools/runtime-profiler/bin/runtime-profiler"
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, "scripts/load_smoke.py", "--profiler", str(profiler)], cwd=ROOT, env={"PATH": directory}, capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(json.loads(result.stdout)["status"], "unavailable")

    def test_sigint_waits_for_the_private_database_teardown(self):
        self.assert_cancellation_cleanup(signal.SIGINT)

    def test_sigterm_waits_for_the_private_database_teardown(self):
        self.assert_cancellation_cleanup(signal.SIGTERM)

    def assert_cancellation_cleanup(self, cancellation_signal):
        docker = ["docker", "--host", "unix:///var/run/docker.sock"]
        def projects(all_containers=False):
            arguments = ["ps"] + (["--all"] if all_containers else [])
            result = subprocess.run(docker + arguments + ["--filter", "name=social-load-", "--format", '{{.Label "com.docker.compose.project"}}'], check=True, capture_output=True, text=True, timeout=10)
            return set(result.stdout.splitlines())
        before = projects(all_containers=True)
        profiler = ROOT / ".artifacts/tools/runtime-profiler/bin/runtime-profiler"
        child = subprocess.Popen([sys.executable, "scripts/load_smoke.py", "--profiler", str(profiler)], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        observed = set()
        try:
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline and child.poll() is None:
                observed = projects() - before
                if observed:
                    break
                time.sleep(0.1)
            self.assertTrue(observed, "capture must reach real database startup")
            for project in observed:
                containers = subprocess.run(docker + ["ps", "--filter", "label=com.docker.compose.project=" + project, "--format", "{{.ID}}"], check=True, capture_output=True, text=True, timeout=10)
                self.assertEqual(len(containers.stdout.splitlines()), 1)
                storage = subprocess.run(docker + ["inspect", containers.stdout.strip(), "--format", "{{json .HostConfig.Tmpfs}}"], check=True, capture_output=True, text=True, timeout=10)
                self.assertIn("size=134217728", json.loads(storage.stdout)["/var/lib/postgresql/data"])
            child.send_signal(cancellation_signal)
            output, errors = child.communicate(timeout=45)
            self.assertEqual(child.returncode, 1, errors)
            self.assertEqual(json.loads(output)["status"], "failed")
            self.assertFalse(observed & projects(all_containers=True), "owned containers must be gone before cancellation returns")
            for project in observed:
                volumes = subprocess.run(docker + ["volume", "ls", "--filter", "label=com.docker.compose.project=" + project, "--format", "{{.Name}}"], check=True, capture_output=True, text=True, timeout=10)
                self.assertEqual(volumes.stdout.strip(), "", "owned volumes must be removed")
        finally:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
                child.communicate(timeout=45)

    def test_process_scenario_cannot_masquerade_as_service_load(self):
        scenario = {
            "schema_version": "runtime-profiler/scenario/v1", "id": "not-service-load",
            "target": {"type": "command", "program": sys.executable, "args": ["-c", "pass"]},
            "run": {"warmup_iterations": 0, "measurement_iterations": 1, "timeout_seconds": 3},
            "collectors": ["process"],
        }
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", dir=ROOT / ".artifacts") as source:
            json.dump(scenario, source)
            source.flush()
            result, report = self.invoke("--scenario", source.name)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(report["status"], "failed")
        self.assertNotIn("bundle", report)

    def test_missing_or_old_compose_is_unavailable_before_capture(self):
        profiler = ROOT / ".artifacts/tools/runtime-profiler/bin/runtime-profiler"
        for version_response in ["exit 1", "echo 2.20.0"]:
            with tempfile.TemporaryDirectory() as directory:
                for tool in ["curl", "cargo"]:
                    os.symlink(shutil.which(tool), Path(directory) / tool)
                docker = Path(directory) / "docker"
                docker.write_text('#!/bin/sh\ncase "$3" in\ninfo) echo 29.8.1 ;;\ncompose) if [ "$4" = version ]; then ' + version_response + '; else exit 1; fi ;;\n*) exit 1 ;;\nesac\n')
                docker.chmod(0o755)
                result = subprocess.run([sys.executable, "scripts/load_smoke.py", "--profiler", str(profiler)], cwd=ROOT, env={"PATH": directory}, capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 2, result.stdout)
            self.assertEqual(json.loads(result.stdout)["status"], "unavailable")

    def test_missing_profiler_is_explicitly_unavailable_without_starting_database(self):
        result = subprocess.run(["python3", "scripts/load_smoke.py", "--profiler", "/missing-social-load-profiler"], cwd=ROOT, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(json.loads(result.stdout)["status"], "unavailable")

if __name__ == "__main__":
    unittest.main()
