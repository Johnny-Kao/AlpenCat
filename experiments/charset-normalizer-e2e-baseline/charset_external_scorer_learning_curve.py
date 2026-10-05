from __future__ import annotations

import hashlib
import json
from collections import Counter, defaultdict

import numpy as np

import charset_external_scorer_domain_shift_ab as base

TRAIN_SIZES = (0, 25, 50, 100, 200, 300)
FOLDS = 4
TOPK = (1, 3, 5)

# Slightly larger one-shot external sample; still intentionally bounded.
base.N_WARC_FILES = 48
base.MAX_ACCEPTED = 520
base.MAX_ROUTE = {"U": 120, "N": 40, "R": 360}
base.TARGET_RESIDUAL = 300


def sample_key(s):
    raw = (
        s["warc_path"] + "\n" + s.get("host", "") + "\n" +
        s["label"] + "\n" + s["route"] + "\n" +
        hashlib.sha256(s["data"][:4096]).hexdigest()
    ).encode()
    return hashlib.sha256(raw).digest()


def deterministic_nested_subset(rows, n):
    if n <= 0:
        return []
    ordered = sorted(rows, key=sample_key)
    return ordered[: min(n, len(ordered))]


def fit_route_models(ext_rows, legacy_X, legacy_y, legacy_b):
    models = {}
    for route in ("U", "N", "RL", "RH"):
        li = np.where(legacy_b == route)[0]
        Xtr = legacy_X[li]
        ytr = legacy_y[li]

        er = [s for s in ext_rows if s["route"] == route]
        if er:
            Xext = np.stack([base.scorer_features(s["data"]) for s in er])
            yext = np.array([s["label"] for s in er], dtype=object)
            known = np.isin(yext, np.unique(legacy_y))
            if known.any():
                Xtr = np.concatenate([Xtr, Xext[known]], axis=0)
                ytr = np.concatenate([ytr, yext[known]], axis=0)

        if len(Xtr) >= 20 and len(np.unique(ytr)) >= 2:
            models[route] = base.fit_linear(Xtr, ytr, C=0.5)
    return models


def evaluate(models, test_rows):
    hits = Counter()
    n = 0
    route_n = Counter()
    route_hits = defaultdict(Counter)

    for s in test_rows:
        model = models.get(s["route"])
        if model is None:
            continue
        if s["label"] not in model[1].classes_:
            continue
        rank = base.rank_model(model, base.scorer_features(s["data"]))
        n += 1
        route_n[s["route"]] += 1
        for k in TOPK:
            h = int(s["label"] in rank[:k])
            hits[k] += h
            route_hits[s["route"]][k] += h

    return {
        "n": n,
        "top1": hits[1] / max(1, n),
        "top3": hits[3] / max(1, n),
        "top5": hits[5] / max(1, n),
        "score": (4 * hits[1] + 2 * hits[3] + hits[5]) / max(1, n),
        "route_metrics": {
            r: {
                "n": route_n[r],
                **{f"top{k}": route_hits[r][k] / max(1, route_n[r]) for k in TOPK},
            }
            for r in sorted(route_n)
        },
    }


def main():
    external, paths, stats = base.collect_external()
    legacy_X, legacy_y, legacy_b = base.load_legacy_training()

    used_paths = sorted(set(s["warc_path"] for s in external))
    fold_assign = {
        p: int.from_bytes(hashlib.sha256(("learning-curve:" + p).encode()).digest()[:8], "big") % FOLDS
        for p in used_paths
    }

    fold_results = []
    pooled_acc = {
        size: {"weighted": Counter(), "n": 0, "actual_train_sizes": []}
        for size in TRAIN_SIZES
    }

    for fold in range(FOLDS):
        train_pool = [s for s in external if fold_assign[s["warc_path"]] != fold]
        test_rows = [s for s in external if fold_assign[s["warc_path"]] == fold]
        per_size = {}

        for requested in TRAIN_SIZES:
            ext_train = deterministic_nested_subset(train_pool, requested)
            models = fit_route_models(ext_train, legacy_X, legacy_y, legacy_b)
            metrics = evaluate(models, test_rows)
            metrics["requested_external_train_n"] = requested
            metrics["actual_external_train_n"] = len(ext_train)
            per_size[str(requested)] = metrics

            n = metrics["n"]
            pooled_acc[requested]["n"] += n
            pooled_acc[requested]["actual_train_sizes"].append(len(ext_train))
            for m in ("top1", "top3", "top5", "score"):
                pooled_acc[requested]["weighted"][m] += metrics[m] * n

        fold_results.append({
            "fold": fold,
            "train_pool_n": len(train_pool),
            "test_n_raw": len(test_rows),
            "results": per_size,
        })

    pooled = {}
    for size in TRAIN_SIZES:
        n = max(1, pooled_acc[size]["n"])
        pooled[str(size)] = {
            "n": pooled_acc[size]["n"],
            "actual_train_sizes_by_fold": pooled_acc[size]["actual_train_sizes"],
            **{
                m: pooled_acc[size]["weighted"][m] / n
                for m in ("top1", "top3", "top5", "score")
            },
        }

    # Simple plateau signal: marginal Top-1 gain between adjacent requested sizes.
    plateau = []
    prev = None
    for size in TRAIN_SIZES:
        cur = pooled[str(size)]
        if prev is not None:
            plateau.append({
                "from": prev,
                "to": size,
                "delta_top1": cur["top1"] - pooled[str(prev)]["top1"],
                "delta_top3": cur["top3"] - pooled[str(prev)]["top3"],
                "delta_top5": cur["top5"] - pooled[str(prev)]["top5"],
            })
        prev = size

    print(json.dumps({
        "phase": "external_scorer_learning_curve",
        "crawl": base.CRAWL,
        "selector": {"S1": "utf8 validity", "S2": "NUL presence", "S3": base.S3},
        "representation": "same 518-dim H/M/T scorer",
        "external_n": len(external),
        "external_routes": dict(Counter(s["route"] for s in external)),
        "external_labels": dict(Counter(s["label"] for s in external).most_common()),
        "external_tiers": dict(Counter(s["tier"] for s in external)),
        "warc_paths_used_n": len(used_paths),
        "collection_stats": dict(stats),
        "train_sizes": TRAIN_SIZES,
        "fold_results": fold_results,
        "pooled": pooled,
        "marginal_gains": plateau,
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
