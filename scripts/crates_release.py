#!/usr/bin/env python3
"""Validate reviewed public packages and verify immutable registry readback."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tomllib
import urllib.error
import urllib.request

NAME = re.compile(r"[A-Za-z0-9_-]+")
VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?")
MAX_CRATE_BYTES = 10 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_json(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "Duplicate JSON key")
            result[key] = value
        return result
    return json.loads(Path(path).read_text(), object_pairs_hook=unique)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def load_plan(root):
    plan = read_json(root / "release/crates.json")
    require(set(plan) == {"schema", "repository", "owner", "toolchain", "packages"}, "Unexpected plan fields")
    require(type(plan["schema"]) is int and plan["schema"] == 1, "Unsupported plan")
    require(re.fullmatch(r"terraphim/[A-Za-z0-9_.-]+", plan["repository"]), "Unexpected repository")
    require(NAME.fullmatch(plan["owner"]) is not None, "Invalid owner")
    require(re.fullmatch(r"[0-9]+\.[0-9]+(?:\.[0-9]+)?", plan["toolchain"]), "Unpinned toolchain")
    require(isinstance(plan["packages"], list) and plan["packages"], "Empty package allowlist")
    names = set()
    for item in plan["packages"]:
        require(set(item) == {"name", "version", "path"}, "Unexpected package fields")
        require(NAME.fullmatch(item["name"]) and VERSION.fullmatch(item["version"]), "Invalid package identity")
        require(item["name"] not in names, "Duplicate package")
        names.add(item["name"])
        require(item["path"] == "crates/" + item["name"] + "/Cargo.toml", "Unexpected manifest path")
    return plan


def selected_package(plan, tag):
    matches = [p for p in plan["packages"] if tag == p["name"] + "-v" + p["version"]]
    require(len(matches) == 1, "Tag is not an allowed exact package version")
    return matches[0]


def validate_identity(root, plan, tag, env):
    item = selected_package(plan, tag)
    require(env.get("GITHUB_REPOSITORY") == plan["repository"], "Wrong repository")
    require(env.get("GITHUB_EVENT_NAME") in {"push", "workflow_dispatch"}, "Unsupported event")
    require(env.get("GITHUB_REF") == "refs/tags/" + tag, "A real release tag is required")
    head = git(root, "rev-parse", "HEAD")
    require(env.get("GITHUB_SHA") == head, "Workflow/source mismatch")
    require(git(root, "rev-parse", "refs/tags/" + tag + "^{commit}") == head, "Moved tag")
    subprocess.run(["git", "-C", str(root), "merge-base", "--is-ancestor", head, "refs/remotes/origin/main"], check=True)
    require(not git(root, "status", "--porcelain"), "Dirty source tree")
    return item, head


def resolved(value, workspace, key):
    return workspace[key] if isinstance(value, dict) and value.get("workspace") is True else value


def dependencies(manifest):
    for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
        yield kind, manifest.get(kind, {})
    for target in manifest.get("target", {}).values():
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            yield kind, target.get(kind, {})


def validate_manifests(root, plan):
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    require(not manifest.get("patch") and not manifest.get("replace"), "Release source overrides are forbidden")
    config_path = root / ".cargo/config.toml"
    if config_path.exists():
        config = tomllib.loads(config_path.read_text())
        require(not config.get("source") and config.get("registry", {}).get("default", "crates-io") == "crates-io", "Registry source override is forbidden")
    workspace = manifest.get("workspace", {})
    order = {p["name"]: i for i, p in enumerate(plan["packages"])}
    versions = {p["name"]: p["version"] for p in plan["packages"]}
    for item in plan["packages"]:
        path = root / item["path"]
        require(path.resolve().is_relative_to(root.resolve()) and not path.is_symlink(), "Manifest escapes checkout")
        data = tomllib.loads(path.read_text())
        package = {k: resolved(v, workspace.get("package", {}), k) for k, v in data["package"].items()}
        require(package["name"] == item["name"] and package["version"] == item["version"], "Manifest/version mismatch")
        require(package.get("publish", ["crates-io"]) == ["crates-io"], "Package is not restricted to crates.io")
        require(package.get("repository") == "https://github.com/" + plan["repository"], "Package repository is not the public source")
        require(package.get("description") and (package.get("license") or package.get("license-file")), "Missing package metadata")
        for kind, table in dependencies(data):
            for name, dep in table.items():
                dep = resolved(dep, workspace.get("dependencies", {}), name)
                if isinstance(dep, str):
                    dep = {"version": dep}
                require("git" not in dep and dep.get("registry", "crates-io") == "crates-io", "Dependency requires a non-public source")
                actual = dep.get("package", name)
                if actual in order:
                    require(order[actual] < order[item["name"]], "Dependency order is invalid")
                    require(dep.get("version", "").removeprefix("=") == versions[actual], "Dependency version differs from plan")
                elif "path" in dep and kind != "dev-dependencies":
                    require(dep.get("version"), "Path dependency has no public version")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def registry_get(path):
    require(path.startswith("/api/v1/crates/"), "Unexpected registry path")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    request = urllib.request.Request("https://crates.io" + path, headers={"User-Agent": "terraphim-release (https://github.com/terraphim/terraphim-core)"})
    try:
        with opener.open(request, timeout=30) as response:
            data = response.read(4 * 1024 * 1024 + 1)
            require(len(data) <= 4 * 1024 * 1024, "Registry response too large")
            return response.status, json.loads(data)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return 404, {}
        raise ValueError("Registry read failed") from None


def validate_registry(plan, item, mode):
    require(mode in {"trusted", "bootstrap"}, "Unknown authentication mode")
    status, _ = registry_get("/api/v1/crates/" + item["name"])
    require(status == (404 if mode == "bootstrap" else 200), "Authentication mode does not match crate existence")
    if mode == "trusted":
        status, owners = registry_get("/api/v1/crates/" + item["name"] + "/owners")
        require(status == 200 and any(p.get("login") == plan["owner"] for p in owners.get("users", [])), "Expected owner not found")
    status, _ = registry_get("/api/v1/crates/" + item["name"] + "/" + item["version"])
    require(status == 404, "Version already exists; inspect immutable output before retrying")
    for previous in plan["packages"][:plan["packages"].index(item)]:
        status, data = registry_get("/api/v1/crates/" + previous["name"] + "/" + previous["version"])
        require(status == 200 and data.get("version", {}).get("yanked") is False, "Publish dependencies first in reviewed order")


def inspect_package(path, item, source_commit):
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= MAX_CRATE_BYTES, "Invalid package archive")
    prefix = item["name"] + "-" + item["version"]
    names, unpacked = [], 0
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            parts = PurePosixPath(member.name).parts
            require(member.isfile() and len(parts) > 1 and parts[0] == prefix and all(p not in {".", ".."} for p in parts), "Unsafe package member")
            require(member.name not in names and len(names) < 10000, "Duplicate or excessive package members")
            require(not any(p in {".env", ".credentials", "credentials.toml", ".git"} for p in parts), "Forbidden package content")
            unpacked += member.size
            require(unpacked <= 100 * 1024 * 1024, "Package expands beyond limit")
            names.append(member.name)
        normalized = tomllib.loads(archive.extractfile(prefix + "/Cargo.toml").read().decode())
        require(normalized["package"]["name"] == item["name"] and normalized["package"]["version"] == item["version"], "Archive identity differs")
        for _, table in dependencies(normalized):
            for dep in table.values():
                if isinstance(dep, dict):
                    require(not any(k in dep for k in ("git", "path", "registry", "registry-index")), "Packaged dependency is not crates.io-only")
        provenance = json.load(archive.extractfile(prefix + "/.cargo_vcs_info.json"))
        require(provenance.get("git", {}).get("sha1") == source_commit and not provenance.get("git", {}).get("dirty", False), "Archive source commit differs")
        require(provenance.get("path_in_vcs") == str(PurePosixPath(item["path"]).parent), "Archive source path differs")
    require(prefix + "/.cargo_vcs_info.json" in names, "Missing Cargo source provenance")
    return {"sha256": digest(path), "bytes": path.stat().st_size, "files": sorted(names)}


def download_readback(item, expected):
    url = "https://static.crates.io/crates/" + item["name"] + "/" + item["name"] + "-" + item["version"] + ".crate"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open(urllib.request.Request(url, headers={"User-Agent": "terraphim-release (https://github.com/terraphim/terraphim-core)"}), timeout=30) as response:
        raw = response.read(MAX_CRATE_BYTES + 1)
        require(response.status == 200 and len(raw) <= MAX_CRATE_BYTES, "Registry download failed")
    require(hashlib.sha256(raw).hexdigest() == expected["sha256"] and len(raw) == expected["bytes"], "Downloaded crate differs from qualified archive")
    return {"url": url, "sha256": expected["sha256"], "bytes": len(raw)}


def unpack_package(archive, item, source_commit, destination, source_root):
    """Validate first, then copy regular members into a new external directory."""
    inspect_package(archive, item, source_commit)
    require(not destination.exists() and not destination.resolve().is_relative_to(source_root.resolve()), "Unpacked tests require a new directory outside the source workspace")
    destination.mkdir(parents=True)
    with tarfile.open(archive, "r:gz") as package:
        for member in package:
            target = destination / member.name
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as output:
                output.write(package.extractfile(member).read())
    return destination / (item["name"] + "-" + item["version"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["validate", "package", "unpack", "compare", "readback"])
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--tag", required=True)
    parser.add_argument("--mode", choices=["trusted", "bootstrap"], default="trusted")
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--expected", type=Path)
    parser.add_argument("--unpack-dir", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    plan = load_plan(root)
    item, head = validate_identity(root, plan, args.tag, os.environ)
    validate_manifests(root, plan)
    receipt = {"schema": 1, "status": "PASS", "repository": plan["repository"], "sourceCommit": head,
               "tag": args.tag, "package": item, "planSha256": digest(root / "release/crates.json")}
    if args.command == "validate":
        validate_registry(plan, item, args.mode)
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                output.write("crate=" + item["name"] + "\nversion=" + item["version"] + "\ntoolchain=" + plan["toolchain"] + "\n")
    else:
        archive = root / "target/package" / (item["name"] + "-" + item["version"] + ".crate")
        receipt["archive"] = inspect_package(archive, item, head)
        if args.command == "unpack":
            require(args.unpack_dir is not None, "Missing isolated unpack directory")
            unpack_package(archive, item, head, args.unpack_dir, root)
        if args.command in {"compare", "readback"}:
            require(args.expected is not None and read_json(args.expected) == receipt, "Qualified package bytes/source changed")
        if args.command == "readback":
            status, data = registry_get("/api/v1/crates/" + item["name"] + "/" + item["version"])
            version = data.get("version", {})
            require(status == 200 and version.get("checksum") == receipt["archive"]["sha256"] and version.get("yanked") is False, "Published checksum/readback differs")
            receipt["registryReadback"] = {"checksum": version["checksum"], "yanked": False}
            receipt["downloadReadback"] = download_readback(item, receipt["archive"])
    args.receipt.write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
