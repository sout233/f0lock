"""Install only the AU on an isolated macOS CI runner and run Apple's validator."""

import subprocess
import sys
from pathlib import Path

from package import ROOT, validate_audio_unit


def main():
    if sys.platform != "darwin":
        raise SystemExit("Native Audio Unit validation requires macOS")

    component = ROOT / "target/bundled/f0lock.component"
    validate_audio_unit(component)
    destination = Path("/Library/Audio/Plug-Ins/Components/f0lock.component")
    subprocess.run(["sudo", "ditto", str(component), str(destination)], check=True)
    subprocess.run(["plutil", "-p", str(destination / "Contents/Info.plist")], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(destination)], check=True)
    inspector = ROOT / "target/au-inspect"
    subprocess.run(["xcrun", "clang++", "-std=c++17", str(ROOT / "tools/ci/inspect_au.cpp"),
                    "-framework", "CoreFoundation", "-o", str(inspector)], check=True)
    subprocess.run([str(inspector), str(destination)], check=True)

    # AU discovery is cached by a separate process. A direct file copy does not
    # guarantee an immediate rescan; restart the registrar before enumeration.
    registrar = subprocess.run(["pgrep", "-x", "AudioComponentRegistrar"], capture_output=True, text=True)
    if registrar.returncode == 0:
        subprocess.run(["sudo", "killall", "-9", "AudioComponentRegistrar"], check=True)
        print("Restarted AudioComponentRegistrar for AU discovery", flush=True)
    elif registrar.returncode == 1:
        print("AudioComponentRegistrar is not running; AU enumeration will start it", flush=True)
    else:
        registrar.check_returncode()

    subprocess.run(["auval", "-a"], check=True)
    subprocess.run(["auval", "-strict", "-v", "aufx", "F0lk", "Sout"], check=True)


if __name__ == "__main__":
    main()
