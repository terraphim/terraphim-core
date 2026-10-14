"""Release refusals tested with real Git histories, TOML files and tar archives."""
import copy
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release", ROOT / "scripts/crates_release.py")
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)


class ReleaseContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.plan = r.load_plan(ROOT)
        (self.root / "release").mkdir()
        (self.root / "release/crates.json").write_text(json.dumps(self.plan))
        (self.root / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/*"]\n')
        for index, package in enumerate(self.plan["packages"]):
            path = self.root / package["path"]
            path.parent.mkdir(parents=True)
            content = '[package]\nname = "' + package["name"] + '"\nversion = "' + package["version"] + '"\npublish = ["crates-io"]\nrepository = "https://github.com/terraphim/terraphim-core"\ndescription = "Release fixture"\nlicense = "Apache-2.0"\n'
            if index:
                dependency = self.plan["packages"][index - 1]
                content += '\n[dependencies]\n"' + dependency["name"] + '" = { version = "' + dependency["version"] + '", path = "../' + dependency["name"] + '" }\n'
            path.write_text(content)
        self.git("init", "-q")
        self.git("config", "user.name", "Release Test")
        self.git("config", "user.email", "release-test@example.invalid")
        self.commit()
        self.head = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", self.head)
        self.item = self.plan["packages"][0]
        self.tag = self.item["name"] + "-v" + self.item["version"]
        self.git("tag", self.tag)
        self.env = {"GITHUB_REPOSITORY": self.plan["repository"], "GITHUB_REF": "refs/tags/" + self.tag,
                    "GITHUB_SHA": self.head, "GITHUB_EVENT_NAME": "push"}

    def git(self, *args):
        return r.git(self.root, *args)

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "Fixture")

    def test_reviewed_exact_tag_and_order_pass(self):
        self.assertEqual(r.validate_identity(self.root, self.plan, self.tag, self.env), (self.item, self.head))
        r.validate_manifests(self.root, self.plan)

    def test_wrong_event_repository_ref_sha_or_version_refused(self):
        cases = {"GITHUB_EVENT_NAME": "pull_request_target", "GITHUB_REPOSITORY": "other/core",
                 "GITHUB_REF": "refs/heads/main", "GITHUB_SHA": "0" * 40}
        for key, value in cases.items():
            with self.subTest(key=key), self.assertRaises(ValueError):
                r.validate_identity(self.root, self.plan, self.tag, {**self.env, key: value})
        with self.assertRaises(ValueError):
            r.selected_package(self.plan, self.tag + "-other")

    def test_untracked_source_refused(self):
        (self.root / "unreviewed.rs").write_text("// unreviewed")
        with self.assertRaisesRegex(ValueError, "Dirty"):
            r.validate_identity(self.root, self.plan, self.tag, self.env)

    def test_source_outside_main_refused(self):
        (self.root / "new.txt").write_text("change")
        self.commit()
        new = self.git("rev-parse", "HEAD")
        self.git("tag", "-f", self.tag)
        with self.assertRaises(subprocess.CalledProcessError):
            r.validate_identity(self.root, self.plan, self.tag, {**self.env, "GITHUB_SHA": new})

    def test_moved_tag_refused(self):
        (self.root / "new.txt").write_text("change")
        self.commit()
        new = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", new)
        with self.assertRaisesRegex(ValueError, "Moved"):
            r.validate_identity(self.root, self.plan, self.tag, {**self.env, "GITHUB_SHA": new})

    def test_manifest_version_and_private_publish_refused(self):
        path = self.root / self.item["path"]
        original = path.read_text()
        for before, after in [(self.item["version"], "99.0.0"), ('["crates-io"]', '["terraphim"]'),
                              ('https://github.com/terraphim/terraphim-core', 'https://git.terraphim.cloud/terraphim/terraphim-core')]:
            path.write_text(original.replace(before, after))
            with self.subTest(after=after), self.assertRaises(ValueError):
                r.validate_manifests(self.root, self.plan)

    def test_reversed_dependency_order_refused(self):
        bad = copy.deepcopy(self.plan)
        bad["packages"].reverse()
        with self.assertRaisesRegex(ValueError, "order"):
            r.validate_manifests(self.root, bad)

    def test_private_registry_git_and_patch_refused(self):
        path = self.root / self.item["path"]
        original = path.read_text()
        for dep in ['registry = "terraphim", version = "1"', 'git = "https://example.invalid/repo"']:
            path.write_text(original + '\n[dependencies]\nother = {' + dep + '}\n')
            with self.assertRaisesRegex(ValueError, "non-public"):
                r.validate_manifests(self.root, self.plan)
        path.write_text(original)
        with (self.root / "Cargo.toml").open("a") as output:
            output.write('\n[patch.crates-io]\nother = { path = "elsewhere" }\n')
        with self.assertRaisesRegex(ValueError, "overrides"):
            r.validate_manifests(self.root, self.plan)

    def test_source_replacement_refused(self):
        (self.root / ".cargo").mkdir()
        (self.root / ".cargo/config.toml").write_text('[source.crates-io]\nreplace-with = "private"\n')
        with self.assertRaisesRegex(ValueError, "override"):
            r.validate_manifests(self.root, self.plan)

    def test_plan_duplicates_and_escape_refused(self):
        path = self.root / "release/crates.json"
        path.write_text('{"schema":1,"schema":1}')
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            r.load_plan(self.root)
        bad = copy.deepcopy(self.plan)
        bad["packages"][0]["path"] = "../Cargo.toml"
        path.write_text(json.dumps(bad))
        with self.assertRaisesRegex(ValueError, "manifest path"):
            r.load_plan(self.root)

    def package(self, extra=None, source=None, dependency=""):
        archive_path = self.root / "test.crate"
        prefix = self.item["name"] + "-" + self.item["version"]
        content = {'Cargo.toml': '[package]\nname="' + self.item["name"] + '"\nversion="' + self.item["version"] + '"\n' + dependency,
                   '.cargo_vcs_info.json': json.dumps({'git': {'sha1': source or self.head}, 'path_in_vcs': str(Path(self.item['path']).parent)}),
                   'src/lib.rs': 'pub fn example() {}'}
        with tarfile.open(archive_path, "w:gz") as archive:
            for name, text in content.items():
                info = tarfile.TarInfo(prefix + "/" + name)
                raw = text.encode(); info.size = len(raw)
                archive.addfile(info, io.BytesIO(raw))
            if extra:
                info = tarfile.TarInfo(extra); info.size = 1
                archive.addfile(info, io.BytesIO(b"x"))
        return archive_path

    def test_real_archive_source_hash_and_list_pass(self):
        path = self.package()
        result = r.inspect_package(path, self.item, self.head)
        self.assertEqual(result["sha256"], r.digest(path))
        self.assertEqual(len(result["files"]), 3)

    def test_real_archive_unsafe_paths_duplicate_secrets_and_source_refused(self):
        prefix = self.item["name"] + "-" + self.item["version"]
        for name in ["../outside", "/absolute", prefix + "/.env", prefix + "/src/lib.rs"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                r.inspect_package(self.package(extra=name), self.item, self.head)
        with self.assertRaisesRegex(ValueError, "source commit"):
            r.inspect_package(self.package(source="0" * 40), self.item, self.head)

    def test_packaged_private_dependency_refused(self):
        path = self.package(dependency='\n[dependencies.other]\nversion="1"\nregistry-index="sparse+https://example.invalid/"\n')
        with self.assertRaisesRegex(ValueError, "crates.io-only"):
            r.inspect_package(path, self.item, self.head)

    def test_unpacked_package_is_external_and_exact(self):
        package = self.package()
        with tempfile.TemporaryDirectory() as external:
            destination = Path(external) / "unpacked"
            crate = r.unpack_package(package, self.item, self.head, destination, self.root)
            self.assertEqual((crate / "src/lib.rs").read_text(), "pub fn example() {}")
            self.assertFalse((crate / ".cargo/config.toml").exists())
            with self.assertRaisesRegex(ValueError, "new directory"):
                r.unpack_package(package, self.item, self.head, destination, self.root)
        with self.assertRaisesRegex(ValueError, "outside"):
            r.unpack_package(package, self.item, self.head, self.root / "unpacked", self.root)

    def test_unsafe_archive_is_rejected_before_any_extraction(self):
        package = self.package(extra="../outside")
        with tempfile.TemporaryDirectory() as external:
            destination = Path(external) / "unpacked"
            with self.assertRaises(ValueError):
                r.unpack_package(package, self.item, self.head, destination, self.root)
            self.assertFalse(destination.exists())

    def test_workflow_separates_bootstrap_oidc_and_qualification(self):
        workflow = (ROOT / ".github/workflows/publish-crates.yml").read_text()
        self.assertNotIn("pull_request:", workflow)
        self.assertIn("startsWith(github.ref, 'refs/tags/')", workflow)
        self.assertIn("environment: crates-io-bootstrap", workflow)
        self.assertIn("github.event_name == 'workflow_dispatch' && inputs.auth_mode == 'bootstrap'", workflow)
        self.assertEqual(workflow.count("id-token: write"), 1)
        self.assertEqual(workflow.count("secrets.CARGO_REGISTRY_TOKEN"), 1)
        self.assertIn("rust-lang/crates-io-auth-action@c6f97d42243bad5fab37ca0427f495c86d5b1a18", workflow)
        for forbidden in ["--allow-dirty", "--no-verify", "op read", "crate_list"]:
            self.assertNotIn(forbidden, workflow)
        qualify = workflow.split("  publish:")[0]
        self.assertNotIn("secrets.", qualify)
        self.assertIn("cargo +\"$TOOLCHAIN\" test --locked", qualify)
        self.assertIn("cargo +\"$TOOLCHAIN\" clippy --locked", qualify)
        self.assertIn('rustup target add wasm32-unknown-unknown --toolchain "$TOOLCHAIN"', qualify)
        self.assertIn('cargo +"$TOOLCHAIN" check --locked -p "$CRATE" --target wasm32-unknown-unknown --lib', qualify)
        unpack = qualify.index('scripts/crates_release.py unpack')
        isolated_cd = qualify.index('cd "$RUNNER_TEMP/unpacked/$CRATE-$VERSION"')
        archive_tests = qualify.index('cargo +"$TOOLCHAIN" test --locked --manifest-path Cargo.toml --all-targets')
        qualified = qualify.index('scripts/crates_release.py package')
        self.assertLess(unpack, isolated_cd)
        self.assertLess(isolated_cd, archive_tests)
        self.assertLess(archive_tests, qualified)


if __name__ == "__main__":
    unittest.main()
