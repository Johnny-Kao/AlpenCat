#!/usr/bin/env python3
from __future__ import annotations

import sys
import tomllib
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEPENDENCIES = ROOT / "upstream-dependencies.toml"
RECONCILIATION = ROOT / "upstream-reconciliation.toml"
CARGO_LOCK = ROOT / "Cargo.lock"

REQUIRED = {
    "id",
    "kind",
    "status",
    "ownership",
    "convergence_state",
    "package",
    "version_requirement",
    "resolved_version",
    "upstream_repo",
    "introduced_at",
    "last_validated_at",
    "manifest_paths",
    "wrapper_paths",
    "usage_markers",
    "contract",
    "upgrade_checks",
    "removal_rule",
    "notes",
}

ALLOWED_STATUS = {"active", "removed", "replaced"}
ALLOWED_OWNERSHIP = {
    "external_mature",
    "external_support",
    "compatibility_pin",
    "internal",
}
ALLOWED_CONVERGENCE = {
    "retain_wrapper",
    "review_host_executor",
    "remove_when_constraint_clears",
    "replace_when_justified",
    "internal_owned",
}


def load(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def cargo_packages() -> dict[str, set[str]]:
    lock = load(CARGO_LOCK)
    packages: dict[str, set[str]] = {}
    for package in lock.get("package", []):
        packages.setdefault(package["name"], set()).add(package["version"])
    return packages


def manifest_requirement(path: Path, package: str) -> str | None:
    data = load(path)
    sections = [
        data.get("dependencies", {}),
        data.get("dev-dependencies", {}),
        data.get("build-dependencies", {}),
    ]
    for section in sections:
        if package not in section:
            continue
        value = section[package]
        if isinstance(value, str):
            return value
        if isinstance(value, dict):
            version = value.get("version")
            return version if isinstance(version, str) else None
    return None


def main() -> int:
    dep_data = load(DEPENDENCIES)
    rec_data = load(RECONCILIATION)

    if dep_data.get("schema_version") != 2:
        raise ValueError("upstream-dependencies.toml schema_version must be 2")

    dependencies = dep_data.get("dependency", [])
    reconciliation_ids = {source["id"] for source in rec_data.get("source", [])}
    lock_packages = cargo_packages()

    seen: set[str] = set()
    active = 0
    temporary = 0
    retained = 0

    for dependency in dependencies:
        dep_id = dependency.get("id", "<unknown>")
        missing = REQUIRED - dependency.keys()
        if missing:
            raise ValueError(f"dependency {dep_id}: missing fields: {sorted(missing)}")
        if dep_id in seen:
            raise ValueError(f"duplicate dependency id: {dep_id}")
        seen.add(dep_id)

        status = dependency["status"]
        if status not in ALLOWED_STATUS:
            raise ValueError(f"dependency {dep_id}: invalid status {status}")

        ownership = dependency["ownership"]
        if ownership not in ALLOWED_OWNERSHIP:
            raise ValueError(f"dependency {dep_id}: invalid ownership {ownership}")

        convergence = dependency["convergence_state"]
        if convergence not in ALLOWED_CONVERGENCE:
            raise ValueError(
                f"dependency {dep_id}: invalid convergence_state {convergence}"
            )

        if not dependency["contract"]:
            raise ValueError(f"dependency {dep_id}: empty contract")
        if not dependency["upgrade_checks"]:
            raise ValueError(f"dependency {dep_id}: empty upgrade_checks")
        if not dependency["removal_rule"]:
            raise ValueError(f"dependency {dep_id}: empty removal_rule")

        try:
            introduced_at = date.fromisoformat(dependency["introduced_at"])
            last_validated_at = date.fromisoformat(dependency["last_validated_at"])
        except (TypeError, ValueError) as error:
            raise ValueError(
                f"dependency {dep_id}: invalid introduced_at/last_validated_at date"
            ) from error
        if last_validated_at < introduced_at:
            raise ValueError(
                f"dependency {dep_id}: last_validated_at precedes introduced_at"
            )

        if not dependency["manifest_paths"]:
            raise ValueError(f"dependency {dep_id}: empty manifest_paths")
        for manifest in dependency["manifest_paths"]:
            manifest_path = ROOT / manifest
            if not manifest_path.exists():
                raise ValueError(
                    f"dependency {dep_id}: manifest path does not exist: {manifest}"
                )
            requirement = manifest_requirement(manifest_path, dependency["package"])
            if requirement is None:
                raise ValueError(
                    f"dependency {dep_id}: package {dependency['package']} not declared "
                    f"in {manifest}"
                )
            if requirement != dependency["version_requirement"]:
                raise ValueError(
                    f"dependency {dep_id}: manifest requirement {requirement!r} in "
                    f"{manifest} does not match ledger "
                    f"{dependency['version_requirement']!r}"
                )

        for wrapper in dependency["wrapper_paths"]:
            if not (ROOT / wrapper).exists():
                raise ValueError(
                    f"dependency {dep_id}: wrapper path does not exist: {wrapper}"
                )

        if ownership != "compatibility_pin" and not dependency["usage_markers"]:
            raise ValueError(f"dependency {dep_id}: empty usage_markers")
        wrapper_text = "\n".join(
            (ROOT / wrapper).read_text(encoding="utf-8", errors="replace")
            for wrapper in dependency["wrapper_paths"]
        )
        for marker in dependency["usage_markers"]:
            if marker not in wrapper_text:
                raise ValueError(
                    f"dependency {dep_id}: usage marker not found in wrapper paths: "
                    f"{marker!r}"
                )

        if status == "active":
            active += 1
            package_versions = lock_packages.get(dependency["package"])
            if package_versions is None:
                raise ValueError(
                    f"dependency {dep_id}: active package "
                    f"{dependency['package']} missing from Cargo.lock"
                )
            if dependency["resolved_version"] not in package_versions:
                raise ValueError(
                    f"dependency {dep_id}: resolved_version "
                    f"{dependency['resolved_version']} not present in Cargo.lock; "
                    f"found {sorted(package_versions)}"
                )
            if dep_id not in reconciliation_ids:
                raise ValueError(
                    f"dependency {dep_id}: active dependency missing from "
                    "upstream-reconciliation.toml"
                )

        if ownership == "compatibility_pin":
            temporary += 1
            if convergence != "remove_when_constraint_clears":
                raise ValueError(
                    f"dependency {dep_id}: compatibility pin must converge via "
                    "remove_when_constraint_clears"
                )

        if convergence == "retain_wrapper":
            retained += 1
            if ownership not in {"external_mature", "external_support"}:
                raise ValueError(
                    f"dependency {dep_id}: retain_wrapper requires external ownership"
                )

    unknown_reconciliation = reconciliation_ids - seen
    if unknown_reconciliation:
        raise ValueError(
            "reconciliation sources missing dependency records: "
            f"{sorted(unknown_reconciliation)}"
        )

    print("dependency convergence validation: PASS")
    print(f"dependencies={len(dependencies)}")
    print(f"active={active}")
    print(f"retained_external={retained}")
    print(f"temporary_pins={temporary}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"dependency convergence validation: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
