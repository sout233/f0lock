"""Build a self-contained Universal AUv2 from the existing Universal CLAP."""

import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def main():
    if sys.platform != "darwin":
        raise SystemExit("Audio Unit builds require macOS and Xcode command line tools")

    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))[
        "package"
    ]["version"]
    bundle = ROOT / "target/bundled/f0lock.clap"
    # The wrapper's native build helper loads this Universal binary to discover
    # the real CLAP ID, parameters and GUI. Sign before loading it on Apple Silicon.
    subprocess.run(
        ["lipo", str(bundle / "Contents/MacOS/f0lock"), "-verify_arch", "arm64", "x86_64"],
        check=True,
    )
    subprocess.run(
        ["codesign", "--force", "--sign", "-", "--timestamp=none", str(bundle)],
        check=True,
    )

    build = ROOT / "target/au-build"
    subprocess.run(
        ["cmake", "-S", str(ROOT / "packaging/macos"), "-B", str(build),
         "-G", "Unix Makefiles", "-DCMAKE_BUILD_TYPE=Release",
         f"-DF0LOCK_VERSION={version.split('-')[0].split('+')[0]}"],
        check=True,
    )
    subprocess.run(
        ["cmake", "--build", str(build), "--config", "Release", "--parallel"],
        check=True,
    )
    component = ROOT / "target/bundled/f0lock.component"
    subprocess.run(
        ["lipo", str(component / "Contents/MacOS/f0lock"), "-verify_arch", "arm64", "x86_64"],
        check=True,
    )
    subprocess.run(
        ["codesign", "--force", "--sign", "-", "--timestamp=none", str(component)],
        check=True,
    )
    subprocess.run(
        ["codesign", "--verify", "--deep", "--strict", str(component)],
        check=True,
    )


if __name__ == "__main__":
    main()
