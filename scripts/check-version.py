"""Impede releases com versões divergentes entre Rust, Tauri, npm e tag."""
import json
import os
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
    cwd=root,
))
packages = {package["name"]: package["version"] for package in metadata["packages"]}
versions = {
    "core": packages["reqlivo"],
    "desktop": packages["reqlivo-desktop"],
    "tauri": json.loads((root / "src-tauri/tauri.conf.json").read_text())["version"],
    "npm": json.loads((root / "package.json").read_text())["version"],
    "npm-lock": json.loads((root / "package-lock.json").read_text())["version"],
}
if os.environ.get("GITHUB_REF", "").startswith("refs/tags/"):
    versions["tag"] = os.environ["GITHUB_REF_NAME"].removeprefix("v")
if len(set(versions.values())) != 1:
    raise SystemExit(f"Versões divergentes: {versions}")
print(f"Versão consistente: {versions['core']}")
