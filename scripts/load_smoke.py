"""Invoke native measurement; only correctness of completed transfers gates smoke."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[1]

class Unavailable(Exception):
    pass

def command(argv, timeout):
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        child = subprocess.Popen(argv, cwd=ROOT, stdout=output, stderr=errors, start_new_session=True, env={"PATH": os.environ.get("PATH", "")})
        try:
            code = child.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            # Let native capture own its thirty-second external-resource teardown.
            child.send_signal(signal.SIGINT)
            try:
                child.wait(timeout=35)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=5)
            raise
        output.seek(0)
        data = output.read(1048577)
        if len(data) > 1048576:
            raise ValueError("command output exceeded 1 MiB")
        if code:
            errors.seek(0)
            message = errors.read(4096).decode("utf-8", errors="replace")
            if "unknown field `headers`" in message or "unknown variant `http-workload`" in message:
                raise Unavailable("profiler lacks declared HTTP workload support")
            raise ValueError(f"{Path(argv[0]).name} failed with exit {code}")
        return data.decode("utf-8")

def run(args):
    profiler = shutil.which(args.profiler)
    if profiler is None:
        raise Unavailable("runtime-profiler is missing; install the pinned source separately")
    detection = json.loads(command([profiler, "detect"], 10))
    if not detection.get("collectors", {}).get("http-curl", {}).get("available"):
        raise Unavailable("bounded HTTP collector is unavailable")
    scenario = Path(args.scenario).resolve(strict=True)
    if not scenario.is_relative_to(ROOT):
        raise ValueError("scenario must belong to this repository")
    plan = json.loads(command([profiler, "plan", "--scenario", str(scenario)], 10))
    if not all(item["supported"] for item in plan["collectors"]):
        raise Unavailable("declared HTTP collector is unsupported")
    for tool in ["cargo", "docker"]:
        if shutil.which(tool) is None:
            raise Unavailable(f"{tool} is missing")
    try:
        command(["docker", "--host", "unix:///var/run/docker.sock", "info", "--format", "{{.ServerVersion}}"], 10)
    except ValueError as error:
        raise Unavailable("local Docker daemon is unavailable") from error
    command(["cargo", "build", "--locked", "--offline", "--example", "load_fixture"], 300)
    directory = ROOT / ".artifacts/load-smoke"
    if any(path.is_symlink() for path in [ROOT / ".artifacts", directory]) or not directory.resolve().is_relative_to(ROOT):
        raise ValueError("evidence directory must stay within this repository")
    directory.mkdir(parents=True, exist_ok=True)
    bundle = directory / ("run-" + uuid.uuid4().hex)
    command([profiler, "capture", "--scenario", str(scenario), "--output", str(bundle)], 180)
    validation = json.loads(command([profiler, "validate", "--bundle", str(bundle)], 10))
    if not validation["valid"]:
        raise ValueError("native bundle validation failed")
    metrics = json.loads(command([profiler, "summarize", "--bundle", str(bundle), "--json"], 10))
    samples = metrics["samples"]
    successful = bool(samples) and all(sample["succeeded"] for sample in samples)
    return {"status": "passed" if successful else "failed", "bundle": bundle.relative_to(ROOT).as_posix(), "measuredRequests": len(samples), "metrics": metrics["metrics"], "timingPolicy": "informational"}, 0 if successful else 1

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--profiler", default="runtime-profiler")
    parser.add_argument("--scenario", default=str(ROOT / ".performance/load-smoke.json"))
    args = parser.parse_args()
    try:
        result, code = run(args)
    except KeyboardInterrupt:
        result, code = {"status": "failed", "reason": "capture cancelled"}, 1
    except Unavailable as error:
        result, code = {"status": "unavailable", "reason": str(error)}, 2
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        result, code = {"status": "failed", "reason": str(error)}, 1
    print(json.dumps(result))
    return code

if __name__ == "__main__":
    sys.exit(main())
