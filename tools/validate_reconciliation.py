#!/usr/bin/env python3
from __future__ import annotations

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "upstream-reconciliation.toml"

SOURCE_REQUIRED = {
    "id",
    "upstream_repo",
    "import_mode",
    "baseline_ref",
    "observed_upstream_ref",
    "downstream_owner",
    "sync_state",
    "sync_policy",
    "transform",
}

TRANSFORM_REQUIRED = {
    "id",
    "upstream_unit",
    "downstream_unit",
    "action",
    "canonical_owner",
    "semantic_contract",
    "upstream_change_policy",
    "validation",
}

INTEGRATION_REQUIRED = {
    "id",
    "source_ids",
    "downstream_unit",
    "action",
    "canonical_owner",
    "semantic_contract",
    "upstream_change_policy",
    "validation",
}

ALLOWED_ACTIONS = {
    "retained",
    "connector_wrapper",
    "deduplicated",
    "deduplicate_or_prune",
    "override",
    "replaced",
    "dropped",
    "compatibility_pin",
}


def require_fields(record: dict, required: set[str], label: str) -> None:
    missing = required - record.keys()
    if missing:
        raise ValueError(f"{label}: missing fields: {sorted(missing)}")


def main() -> int:
    with LEDGER.open("rb") as handle:
        data = tomllib.load(handle)

    if data.get("schema_version") != 2:
        raise ValueError("schema_version must be 2")

    policy = data.get("policy", {})
    if policy.get("update_model") != "four_way_reconciliation":
        raise ValueError("policy.update_model must be four_way_reconciliation")
    if policy.get("advance_baseline_only_after_validation") is not True:
        raise ValueError("baseline advancement must require validation")

    sources = data.get("source", [])
    source_ids: set[str] = set()
    transform_ids: set[str] = set()

    for source in sources:
        source_id = source.get("id", "<unknown>")
        require_fields(source, SOURCE_REQUIRED, f"source {source_id}")

        if source_id in source_ids:
            raise ValueError(f"duplicate source id: {source_id}")
        source_ids.add(source_id)

        if not source["baseline_ref"]:
            raise ValueError(f"source {source_id}: baseline_ref is empty")
        if not source["observed_upstream_ref"]:
            raise ValueError(f"source {source_id}: observed_upstream_ref is empty")

        for transform in source["transform"]:
            transform_id = transform.get("id", "<unknown>")
            require_fields(
                transform,
                TRANSFORM_REQUIRED,
                f"source {source_id} transform {transform_id}",
            )
            if transform_id in transform_ids:
                raise ValueError(f"duplicate transform id: {transform_id}")
            transform_ids.add(transform_id)

            if transform["action"] not in ALLOWED_ACTIONS:
                raise ValueError(
                    f"transform {transform_id}: unsupported action {transform['action']}"
                )
            if not transform["validation"]:
                raise ValueError(f"transform {transform_id}: validation is empty")

    integrations = data.get("integration", [])
    integration_ids: set[str] = set()

    for integration in integrations:
        integration_id = integration.get("id", "<unknown>")
        require_fields(
            integration,
            INTEGRATION_REQUIRED,
            f"integration {integration_id}",
        )

        if integration_id in integration_ids:
            raise ValueError(f"duplicate integration id: {integration_id}")
        integration_ids.add(integration_id)

        unknown_sources = set(integration["source_ids"]) - source_ids
        if unknown_sources:
            raise ValueError(
                f"integration {integration_id}: unknown source ids: "
                f"{sorted(unknown_sources)}"
            )

        if integration["action"] not in ALLOWED_ACTIONS:
            raise ValueError(
                f"integration {integration_id}: unsupported action "
                f"{integration['action']}"
            )
        if not integration["validation"]:
            raise ValueError(f"integration {integration_id}: validation is empty")

    print("reconciliation ledger validation: PASS")
    print(f"sources={len(sources)}")
    print(f"source_transforms={len(transform_ids)}")
    print(f"cross_source_integrations={len(integrations)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"reconciliation ledger validation: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
