import argparse
import hashlib
import plistlib
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def validate_binary(path, platform):
    with path.open("rb") as binary:
        if platform == "windows":
            if binary.read(2) != b"MZ":
                raise ValueError(f"Not a Windows binary: {path}")
            binary.seek(0x3C)
            offset = struct.unpack("<I", binary.read(4))[0]
            binary.seek(offset)
            if binary.read(6) != b"PE\0\0\x64\x86":
                raise ValueError(f"Not an x64 PE binary: {path}")
        elif platform == "linux":
            header = binary.read(20)
            if header[:6] != b"\x7fELF\x02\x01" or header[18:20] != b"\x3e\x00":
                raise ValueError(f"Not an x64 ELF binary: {path}")


def populate(stage, platform):
    bundle = ROOT / "target/bundled"
    for name in ["f0lock.clap", "f0lock.vst3"]:
        source = bundle / name
        if source.is_dir():
            shutil.copytree(source, stage / name, symlinks=True)
        else:
            shutil.copy2(source, stage / name)

    binary = {
        "windows": "f0lock.vst3/Contents/x86_64-win/f0lock.vst3",
        "linux": "f0lock.vst3/Contents/x86_64-linux/f0lock.so",
        "macos": "f0lock.vst3/Contents/MacOS/f0lock",
    }[platform]
    if not (stage / binary).is_file():
        raise FileNotFoundError(stage / binary)

    if platform != "macos":
        for path in [stage / "f0lock.clap", stage / binary]:
            validate_binary(path, platform)
        with (
            (stage / "f0lock.clap").open("rb") as clap,
            (stage / binary).open("rb") as vst3,
        ):
            if (
                hashlib.file_digest(clap, "sha256").digest()
                != hashlib.file_digest(vst3, "sha256").digest()
            ):
                raise ValueError("CLAP and VST3 contain different plugin binaries")

    shutil.copy2(ROOT / "README.md", stage / "README.md")
    shutil.copytree(ROOT / "docs", stage / "docs")
    licenses = stage / "licenses"
    licenses.mkdir()
    sources = {
        "Ubuntu-UFL.txt": ROOT / "assets/fonts/UFL.txt",
        "nice-plug-ISC.txt": ROOT.parent / "f0lock_vendor/nice-plug/LICENSE",
        "pitch-estimate-MIT.txt": ROOT.parent / "f0lock_vendor/pitch-estimate/LICENSE",
    }
    for name in ["baseview", "egui-baseview"]:
        for license_name in ["MIT", "APACHE"]:
            sources[f"{name}-{license_name}.txt"] = (
                ROOT / f"vendor/{name}/LICENSE-{license_name}"
            )
    for name, source in sources.items():
        shutil.copy2(source, licenses / name)

    instructions = {
        "windows": (
            "Run the setup EXE to install both formats for all users.\n"
            "Or copy f0lock.clap to C:/Program Files/Common Files/CLAP/\n"
            "and f0lock.vst3 to C:/Program Files/Common Files/VST3/.\n"
        ),
        "macos": (
            "Requires macOS 14 or newer; supports Apple Silicon and Intel.\n"
            "Run the PKG to install both formats for all users.\n"
            "Or copy the bundles into ~/Library/Audio/Plug-Ins/CLAP/\n"
            "and ~/Library/Audio/Plug-Ins/VST3/ for the current user.\n"
            "These builds are ad-hoc signed, without Developer ID or notarization.\n"
            "The PKG is unsigned; macOS may require approval in Privacy & Security.\n"
            "Audio Units (AU) are not included; Logic Pro requires AU.\n"
        ),
        "linux": (
            "Requires x86_64 Linux, glibc 2.35 or newer, X11 and OpenGL.\n"
            "Run ./install.sh to install into ~/.clap/ and ~/.vst3/.\n"
            "Do not run the per-user installation script with sudo.\n"
            "To uninstall, remove ~/.clap/f0lock.clap and ~/.vst3/f0lock.vst3.\n"
        ),
    }
    (stage / "INSTALL.txt").write_text(
        instructions[platform] + "Rescan plugins in your DAW after installation.\n",
        encoding="utf-8",
    )


def windows_package(stage, output, version, compiler):
    subprocess.run(
        [
            str(compiler),
            f"/DAppVersion={version}",
            f"/DSourceDir={stage}",
            f"/DOutputDir={output}",
            str(ROOT / "packaging/windows/f0lock.iss"),
        ],
        check=True,
    )
    installer = output / f"f0lock-{version}-windows-x64-setup.exe"
    if not installer.is_file():
        raise FileNotFoundError(installer)

    destination = output / f"f0lock-{version}-windows-x64.zip"
    with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(stage.rglob("*")):
            if path.is_file():
                with (
                    path.open("rb") as source,
                    archive.open(path.relative_to(stage).as_posix(), "w") as target,
                ):
                    while chunk := source.read(8192):
                        target.write(chunk)
    with zipfile.ZipFile(destination) as archive:
        if archive.testzip() is not None:
            raise ValueError(f"Corrupt archive: {destination}")


def macos_package(stage, output, version):
    for name in ["f0lock.clap", "f0lock.vst3"]:
        bundle = stage / name
        plist_path = bundle / "Contents/Info.plist"
        with plist_path.open("rb") as source:
            metadata = plistlib.load(source)
        metadata.update(
            CFBundleIdentifier=f"com.sout.f0lock.{name.rsplit('.', 1)[1]}",
            CFBundleShortVersionString=version.split("-")[0].split("+")[0],
            CFBundleVersion=version.split("-")[0].split("+")[0],
            LSMinimumSystemVersion="14.0",
        )
        with plist_path.open("wb") as destination:
            plistlib.dump(metadata, destination)

        subprocess.run(
            [
                "lipo",
                str(bundle / "Contents/MacOS/f0lock"),
                "-verify_arch",
                "x86_64",
                "arm64",
            ],
            check=True,
        )
        subprocess.run(
            ["codesign", "--force", "--sign", "-", "--timestamp=none", str(bundle)],
            check=True,
        )
        subprocess.run(
            ["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True
        )

    payload = stage.parent / "pkg-root"
    for name, directory in [("f0lock.clap", "CLAP"), ("f0lock.vst3", "VST3")]:
        destination = payload / "Library/Audio/Plug-Ins" / directory / name
        shutil.copytree(stage / name, destination, symlinks=True)
    support = payload / "Library/Application Support/f0lock"
    shutil.copytree(stage / "licenses", support / "licenses")
    for name in ["README.md", "INSTALL.txt"]:
        shutil.copy2(stage / name, support / name)

    stem = f"f0lock-{version}-macos-universal"
    components = stage.parent / "components.plist"
    subprocess.run(
        ["pkgbuild", "--analyze", "--root", str(payload), str(components)], check=True
    )
    with components.open("rb") as source:
        bundles = plistlib.load(source)
    for bundle in bundles:
        bundle["BundleIsRelocatable"] = False
    with components.open("wb") as destination:
        plistlib.dump(bundles, destination)

    subprocess.run(
        [
            "pkgbuild",
            "--root",
            str(payload),
            "--identifier",
            "com.sout.f0lock",
            "--version",
            version.split("-")[0].split("+")[0],
            "--install-location",
            "/",
            "--component-plist",
            str(components),
            "--ownership",
            "recommended",
            str(output / f"{stem}.pkg"),
        ],
        check=True,
    )
    subprocess.run(
        [
            "ditto",
            "-c",
            "-k",
            "--sequesterRsrc",
            str(stage),
            str(output / f"{stem}.zip"),
        ],
        check=True,
    )


def linux_package(stage, output, version):
    installer = stage / "install.sh"
    shutil.copy2(ROOT / "packaging/linux/install.sh", installer)
    installer.chmod(0o755)
    stem = f"f0lock-{version}-linux-x64"
    with tarfile.open(output / f"{stem}.tar.gz", "w:gz") as archive:
        archive.add(stage, arcname=stem)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("platform", choices=["windows", "macos", "linux"])
    parser.add_argument("--iscc", type=Path)
    args = parser.parse_args()
    expected_platform = {"windows": "win32", "macos": "darwin", "linux": "linux"}[
        args.platform
    ]
    if sys.platform != expected_platform:
        parser.error(
            f"Packaging {args.platform} requires {expected_platform}, got {sys.platform}"
        )
    if args.platform == "windows" and (args.iscc is None or not args.iscc.is_file()):
        parser.error("Windows packaging requires --iscc pointing to ISCC.exe")

    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))[
        "package"
    ]["version"]
    output = ROOT / "dist"
    output.mkdir(exist_ok=True)
    workspace = (ROOT / "target/release-packaging").resolve()
    if not workspace.is_relative_to(ROOT):
        raise ValueError(
            f"Packaging directory must be inside the repository: {workspace}"
        )
    workspace.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(dir=workspace) as temporary:
        stage = Path(temporary) / "f0lock"
        stage.mkdir()
        populate(stage, args.platform)
        if args.platform == "windows":
            windows_package(stage, output, version, args.iscc.resolve())
        elif args.platform == "macos":
            macos_package(stage, output, version)
        else:
            linux_package(stage, output, version)


if __name__ == "__main__":
    main()
