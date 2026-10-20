import json
import subprocess
from pathlib import Path


def main():
    root = Path(__file__).resolve().parents[2]
    dependencies = json.loads(
        (root / "tools/ci/dependencies.json").read_text(encoding="utf-8")
    )
    vendor = root.parent / "f0lock_vendor"

    for name, dependency in dependencies.items():
        destination = vendor / name
        if destination.exists():
            raise FileExistsError(
                f"Dependency destination already exists: {destination}"
            )

        subprocess.run(["git", "init", str(destination)], check=True)
        git = ["git", "-C", str(destination)]
        subprocess.run(git + ["config", "core.autocrlf", "false"], check=True)
        subprocess.run(git + ["remote", "add", "origin", dependency["url"]], check=True)
        subprocess.run(
            git + ["fetch", "--depth=1", "origin", dependency["revision"]], check=True
        )
        subprocess.run(git + ["checkout", "--detach", "FETCH_HEAD"], check=True)

        revision = subprocess.check_output(
            git + ["rev-parse", "HEAD"], text=True
        ).strip()
        if revision != dependency["revision"]:
            raise RuntimeError(f"Unexpected revision for {name}: {revision}")

        if "patch" in dependency:
            patch = str(root / dependency["patch"])
            subprocess.run(git + ["apply", "--check", patch], check=True)
            subprocess.run(git + ["apply", patch], check=True)


if __name__ == "__main__":
    main()
