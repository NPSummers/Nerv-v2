import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import release_version as version


spec = importlib.util.spec_from_file_location("publish_release", Path(__file__).with_name("publish-release.py"))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class Versions(unittest.TestCase):
    def test_semver_identifiers_and_leading_zeroes(self):
        for value in ["0.1.0", "1.2.3-alpha.10", "1.2.3-beta.0+ci.01", "2.0.0-01a", "1.0.0+001"]:
            version.parse(value)
        for value in ["v1.2.3", "1.2", "01.2.3", "1.02.3", "1.2.03", "1.2.3-01", "1.2.3-alpha..1", "1.2.3-", "1.2.3+", "1.2.3+ci/1", "1.2.3\n", "1.2.3-\u00e9"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                version.parse(value)

    def test_precedence_is_numeric_and_ignores_build_metadata(self):
        ordered = ["1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "1.0.1", "1.10.0"]
        self.assertEqual(sorted(reversed(ordered), key=version.precedence), ordered)
        self.assertEqual(version.precedence("1.2.3+one"), version.precedence("1.2.3+two"))

    def test_tag_manifest_and_lock_must_agree(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / "crates/nerv-cli/Cargo.toml"
            manifest.parent.mkdir(parents=True)
            manifest.write_text('[package]\nname="nerv"\nversion="0.1.0-rc.1"\n', encoding="utf-8")
            lock = root / "Cargo.lock"
            lock.write_text('[[package]]\nname="nerv"\nversion="0.1.0-rc.1"\n', encoding="utf-8")
            self.assertTrue(version.validate(root, "v0.1.0-rc.1")["prerelease"])
            with self.assertRaisesRegex(ValueError, "release tag"):
                version.validate(root, "v0.1.0")
            lock.write_text('[[package]]\nname="nerv"\nversion="0.1.0"\n', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Cargo.lock"):
                version.validate(root)

    def test_numeric_installer_version_does_not_claim_semver_precedence(self):
        for suffix in ["-alpha.1", "-alpha.10", "-rc.1", "", "+build.1"]:
            self.assertEqual(version.installer_version("1.2.3" + suffix), "1.2.3.0")
        with self.assertRaises(ValueError):
            version.installer_version("65536.0.0")

    def test_manual_target_selection_cannot_reduce_a_tag_release_matrix(self):
        command = [sys.executable, str(Path(__file__).with_name("release_version.py")), "--platform", "linux-x64"]
        manual = json.loads(subprocess.check_output(command, text=True))
        self.assertEqual([item["label"] for item in manual["matrix"]["include"]], ["linux-x64"])
        tagged = json.loads(subprocess.check_output([*command, "--tag", "v" + version.validate()["version"]], text=True))
        self.assertEqual({item["label"] for item in tagged["matrix"]["include"]}, {"linux-x64", "macos-arm64", "windows-x64"})


class Publication(unittest.TestCase):
    def setUp(self):
        tag_verifier = patch.object(publisher, "verify_tag")
        tag_verifier.start()
        self.addCleanup(tag_verifier.stop)
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.version = "1.2.3-rc.1"
        self.commit = "a" * 40
        for platform in sorted(publisher.PLATFORMS):
            suffixes = ["portable.tar.gz"] if platform.startswith("linux") else ["portable.zip", "installer.pkg" if platform.startswith("macos") else "setup-unsigned.exe"]
            artifacts = []
            for suffix in suffixes:
                path = self.root / f"nerv-{self.version}-{platform}-{suffix}"
                path.write_bytes(platform.encode())
                artifacts.append({"name": path.name, "size": path.stat().st_size, "sha256": publisher.digest(path)})
            manifest = {"version": self.version, "source_commit": self.commit, "source_dirty": False, "platform": platform, "signing": "Developer ID, notarized" if platform.startswith("macos") else "unsigned", "artifacts": artifacts}
            (self.root / f"manifest-{platform}.json").write_text(json.dumps(manifest), encoding="utf-8")

    def test_inventory_requires_complete_matching_platforms_and_hashes(self):
        assets = publisher.inventory(self.root, self.version, self.commit)
        self.assertEqual(len(assets), 7)
        self.assertEqual(len((self.root / "SHA256SUMS").read_text().splitlines()), 6)
        with self.assertRaisesRegex(ValueError, "source commit"):
            publisher.inventory(self.root, self.version, "b" * 40)
        assets[0].write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "mismatch"):
            publisher.inventory(self.root, self.version, self.commit)

    def test_missing_platform_or_unsigned_macos_blocks_publication(self):
        path = self.root / "manifest-macos-arm64.json"
        manifest = json.loads(path.read_text())
        manifest["signing"] = "unsigned"
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "notarized"):
            publisher.inventory(self.root, self.version, self.commit)
        path.unlink()
        with self.assertRaisesRegex(ValueError, "three required"):
            publisher.inventory(self.root, self.version, self.commit)

    def test_non_tag_events_never_call_github(self):
        for event, ref in [("workflow_dispatch", "refs/tags/v1.2.3-rc.1"), ("push", "refs/heads/main"), ("pull_request", "refs/pull/1/merge")]:
            with self.subTest(event=event), patch.dict(os.environ, {"GITHUB_EVENT_NAME": event, "GITHUB_REF": ref}), patch.object(publisher.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "tag push"):
                    publisher.publish(self.root, "v" + self.version, self.commit)
                run.assert_not_called()

    def test_dirty_source_cannot_be_published(self):
        path = self.root / "manifest-linux-x64.json"
        manifest = json.loads(path.read_text())
        manifest["source_dirty"] = True
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "unmodified tagged checkout"):
            publisher.inventory(self.root, self.version, self.commit)

    def test_new_release_stays_draft_until_all_assets_are_uploaded(self):
        calls = []
        def run(arguments, **kwargs):
            calls.append(arguments)
            return subprocess.CompletedProcess(arguments, 1, "", "HTTP 404") if arguments[1] == "api" else subprocess.CompletedProcess(arguments, 0)
        with patch.dict(os.environ, {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/tags/v" + self.version, "GH_REPO": "owner/repo"}), patch.object(publisher.subprocess, "run", side_effect=run):
            publisher.publish(self.root, "v" + self.version, self.commit)
        self.assertIn("--draft", calls[1])
        self.assertEqual(sum(call[1:3] == ["release", "upload"] for call in calls), 7)
        self.assertIn("--draft=false", calls[-1])
        self.assertIn("--prerelease=true", calls[-1])
        self.assertIn("--latest=false", calls[-1])
        self.assertFalse(any("--clobber" in call for call in calls))

    def test_published_assets_cannot_be_replaced(self):
        assets = publisher.inventory(self.root, self.version, self.commit)
        release = {"draft": False, "assets": [{"name": path.name} for path in assets]}
        def run(arguments, **kwargs):
            if arguments[1] == "api":
                return subprocess.CompletedProcess(arguments, 0, json.dumps(release), "")
            self.assertEqual(arguments[1:3], ["release", "download"])
            (Path(arguments[-1]) / arguments[5]).write_bytes(b"different published bytes")
            return subprocess.CompletedProcess(arguments, 0)
        with patch.dict(os.environ, {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/tags/v" + self.version, "GH_REPO": "owner/repo"}), patch.object(publisher.subprocess, "run", side_effect=run):
            with self.assertRaisesRegex(ValueError, "refusing to replace"):
                publisher.publish(self.root, "v" + self.version, self.commit)

    def test_identical_published_release_retry_does_not_mutate_it(self):
        assets = publisher.inventory(self.root, self.version, self.commit)
        release = {"draft": False, "assets": [{"name": path.name} for path in assets]}
        calls = []
        def run(arguments, **kwargs):
            calls.append(arguments)
            if arguments[1] == "api":
                return subprocess.CompletedProcess(arguments, 0, json.dumps(release), "")
            self.assertEqual(arguments[1:3], ["release", "download"])
            name = arguments[5]
            (Path(arguments[-1]) / name).write_bytes((self.root / name).read_bytes())
            return subprocess.CompletedProcess(arguments, 0)
        with patch.dict(os.environ, {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/tags/v" + self.version, "GH_REPO": "owner/repo"}), patch.object(publisher.subprocess, "run", side_effect=run):
            publisher.publish(self.root, "v" + self.version, self.commit)
        self.assertEqual(len(calls), 8)

    def test_stable_latest_uses_semver_instead_of_release_creation_order(self):
        previous_version = self.version
        self.version = "1.2.3"
        for path in self.root.glob("manifest-*.json"):
            manifest = json.loads(path.read_text())
            manifest["version"] = self.version
            for artifact in manifest["artifacts"]:
                name = artifact["name"].replace(previous_version, self.version)
                (self.root / artifact["name"]).rename(self.root / name)
                artifact["name"] = name
            path.write_text(json.dumps(manifest))
        for previous, expected in [("v1.2.2", "true"), ("v1.2.10", "false")]:
            calls = []
            def run(arguments, **kwargs):
                calls.append(arguments)
                if arguments[1] == "api":
                    if arguments[2].endswith("/releases/latest"):
                        return subprocess.CompletedProcess(arguments, 0, json.dumps({"tag_name": previous}), "")
                    return subprocess.CompletedProcess(arguments, 1, "", "HTTP 404")
                return subprocess.CompletedProcess(arguments, 0)
            with self.subTest(previous=previous), patch.dict(os.environ, {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/tags/v" + self.version, "GH_REPO": "owner/repo"}), patch.object(publisher.subprocess, "run", side_effect=run):
                publisher.publish(self.root, "v" + self.version, self.commit)
            self.assertIn("--prerelease=false", calls[-1])
            self.assertIn(f"--latest={expected}", calls[-1])


class TagIdentity(unittest.TestCase):
    def test_annotated_tag_is_peeled_and_moving_tags_are_rejected(self):
        commit = "a" * 40
        reference = json.dumps({"object": {"type": "tag", "sha": "b" * 40}})
        target = json.dumps({"object": {"type": "commit", "sha": commit}})
        with patch.object(publisher.subprocess, "check_output", side_effect=[reference, target]) as api:
            publisher.verify_tag("owner/repo", "v1.2.3", commit)
            self.assertEqual(api.call_count, 2)
        with patch.object(publisher.subprocess, "check_output", return_value=target):
            with self.assertRaisesRegex(ValueError, "tag moved"):
                publisher.verify_tag("owner/repo", "v1.2.3", "c" * 40)


if __name__ == "__main__":
    unittest.main()
