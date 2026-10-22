"""Portable tests for AU packaging contracts; native validation runs with auval in CI."""

import plistlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import package


class AudioUnitPackagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bundle = self.root / "target/bundled/f0lock.component"
        self.metadata = {
            "CFBundleIdentifier": "com.sout.f0lock.component",
            "AudioComponents": [{
                "type": "aufx", "subtype": "F0lk", "manufacturer": "Sout",
                "name": "sout: f0lock", "factoryFunction": "wrapAsAUV2_inst0Factory",
            }],
        }
        for path in [self.bundle / "Contents/MacOS/f0lock",
                     self.bundle / "Contents/PlugIns/f0lock.clap/Contents/MacOS/f0lock"]:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"test fixture, not an executable")
        self.write_metadata()

    def write_metadata(self):
        with (self.bundle / "Contents/Info.plist").open("wb") as destination:
            plistlib.dump(self.metadata, destination)

    def test_self_contained_effect_has_stable_identity(self):
        package.validate_audio_unit(self.bundle)

    def test_missing_embedded_engine_is_an_error(self):
        (self.bundle / "Contents/PlugIns/f0lock.clap/Contents/MacOS/f0lock").unlink()
        with self.assertRaises(FileNotFoundError):
            package.validate_audio_unit(self.bundle)

    def test_changed_identity_or_missing_factory_is_an_error(self):
        self.metadata["AudioComponents"][0]["subtype"] = "oops"
        self.write_metadata()
        with self.assertRaises(ValueError):
            package.validate_audio_unit(self.bundle)
        self.metadata["AudioComponents"][0]["subtype"] = "F0lk"
        self.metadata["AudioComponents"][0]["factoryFunction"] = ""
        self.write_metadata()
        with self.assertRaises(ValueError):
            package.validate_audio_unit(self.bundle)

    def test_macos_stage_contains_three_formats_licenses_and_logic_instructions(self):
        for name in ["f0lock.clap", "f0lock.vst3"]:
            binary = self.root / "target/bundled" / name / "Contents/MacOS/f0lock"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"test fixture, not an executable")
        sources = ["README.md", "docs/fixture.md", "assets/fonts/UFL.txt",
                   "vendor/nice-plug/LICENSE", "vendor/pitch-estimate/LICENSE"]
        sources += [f"vendor/{name}/LICENSE-{license_name}"
                    for name in ["baseview", "egui-baseview"]
                    for license_name in ["MIT", "APACHE"]]
        sources += [f"target/bundled/au-licenses/{name}"
                    for name in ["clap-wrapper-MIT.txt", "clap-MIT.txt", "AudioUnitSDK.txt"]]
        for name in sources:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("UTF-8 fixture", encoding="utf-8")

        stage = self.root / "stage"
        stage.mkdir()
        with patch.object(package, "ROOT", self.root):
            package.populate(stage, "macos")

        package.validate_audio_unit(stage / "f0lock.component")
        for name, _ in package.MACOS_FORMATS:
            self.assertTrue((stage / name).is_dir())
        for name in ["clap-wrapper-MIT.txt", "clap-MIT.txt", "AudioUnitSDK.txt"]:
            self.assertTrue((stage / "licenses" / name).is_file())
        instructions = (stage / "INSTALL.txt").read_text(encoding="utf-8")
        self.assertIn("Audio Units > sout > f0lock", instructions)
        self.assertIn("Components/", instructions)
        self.assertNotIn("AU) are not included", instructions)


if __name__ == "__main__":
    unittest.main()
