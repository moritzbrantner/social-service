"""Own a private Compose database; runtime-profiler owns this foreground group."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]
DOCKER = ["docker", "--host", "unix:///var/run/docker.sock"]

def command(argv, timeout):
    return subprocess.run(argv, cwd=ROOT, check=True, capture_output=True, text=True, timeout=timeout).stdout.strip()

def run(mode):
    scope = Path(os.environ["RUNTIME_PROFILER_FIXTURE_DIRECTORY"]).resolve(strict=True)
    manifest = scope / "social-compose.json"
    override = scope / "compose-load.yaml"
    if mode == "stop":
        if not manifest.exists():
            return
        project = json.loads(manifest.read_text())["project"]
        if not re.fullmatch(r"social-load-[0-9a-f]{32}", project):
            raise ValueError("invalid owned project")
        command(DOCKER + ["compose", "--project-name", project, "--file", str(ROOT / "compose.yaml"), "--file", str(override), "down", "--timeout", "0", "--volumes", "--remove-orphans"], 25)
        return
    if mode != "start" or manifest.exists():
        raise ValueError("fixture requires a fresh start or owned stop")
    project = "social-load-" + uuid.uuid4().hex
    override.write_text('services:\n  postgres:\n    ports: !override ["127.0.0.1::5432"]\n    volumes: !reset []\n    tmpfs: ["/var/lib/postgresql/data:rw,size=134217728"]\n')
    # Persist ownership before creating resources so failed startup can still clean up.
    manifest.write_text(json.dumps({"project": project}))
    compose = DOCKER + ["compose", "--project-name", project, "--file", str(ROOT / "compose.yaml"), "--file", str(override)]
    command(compose + ["up", "--pull", "never", "--detach", "--wait", "--wait-timeout", "30", "postgres"], 40)
    binding = command(compose + ["port", "postgres", "5432"], 5)
    if not re.fullmatch(r"127\.0\.0\.1:[0-9]+", binding):
        raise ValueError("database binding must be loopback")
    port = int(binding.rsplit(":", 1)[1])
    if not 1 <= port <= 65535:
        raise ValueError("invalid database port")
    environment = {"PATH": os.environ["PATH"], "SOCIAL_LOAD_DATABASE_URL": f"postgres://social:social@127.0.0.1:{port}/social", "RUNTIME_PROFILER_PORT_FILE": os.environ["RUNTIME_PROFILER_PORT_FILE"]}
    binary = ROOT / "target/debug/examples/load_fixture"
    os.execve(binary, [str(binary)], environment)

if __name__ == "__main__":
    try:
        run(sys.argv[1])
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"load fixture failed: {type(error).__name__}", file=sys.stderr)
        sys.exit(1)
