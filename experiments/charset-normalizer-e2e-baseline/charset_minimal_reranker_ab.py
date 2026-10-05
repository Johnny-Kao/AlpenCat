from __future__ import annotations

import hashlib
import json
from collections import Counter

import numpy as np

import charset_external_scorer_learning_curve as lc
import charset_external_scorer_domain_shift_ab as base

FOLDS = 4
TRAIN_N = 300


def has_utf8_bom(data: bytes) -> bool:
    return data.startswith(b"\xef\xbb\xbf")


def ascii_only(data: bytes) -> bool:
    sample = data[:4096]
    return all(b < 128 for b in sample)


def strict_utf8(data: bytes) -> bool:
    try:
        data[:4096].decode("utf-8", "strict")
        return True
    except UnicodeDecodeError:
        return False


def family(x: str | None) -> str:
    if x is None:
        return "none"
    if x.startswith("utf-"):
        return "utf"
    if x.startswith("cp12"):
        return "singlebyte-win"
    if x.startswith("iso-8859") or x.startswith("iso8859"):
        return "singlebyte-iso"
    if x in ("shift-jis", "euc-jp", "euc-kr", "big5", "gb18030", "gbk", "cp949"):
        return "cjk"
    return x


def rerank(data: bytes, rank: list[str]) -> list[str]:
    if not rank:
        return rank
    r = list(rank)

    # Rule 1: BOM is deterministic evidence for UTF-8-SIG.
    if has_utf8_bom(data) and "utf-8-sig" in r:
        r.remove("utf-8-sig")
        r.insert(0, "utf-8-sig")
        return r

    # Rule 2: strict UTF-8-valid non-BOM bytes should prefer UTF-8 over
    # nearby legacy encodings when UTF-8 is already in the candidate set.
    if strict_utf8(data) and "utf-8" in r:
        if r[0] != "utf-8":
            topfam = family(r[0])
            if topfam in ("utf", "cjk", "singlebyte-win", "singlebyte-iso", "ascii"):
                r.remove("utf-8")
                r.insert(0, "utf-8")
        return r

    # Rule 3: ASCII-only content should prefer ASCII when it is already present.
    if ascii_only(data) and "ascii" in r:
        r.remove("ascii")
        r.insert(0, "ascii")
        return r

    # Rule 4: very small family calibration for common single-byte confusions.
    # Only swap within top-3; never introduce a class that scorer did not rank.
    top3 = r[:3]
    if r[0] == "iso-8859-9" and "iso-8859-1" in top3:
        r.remove("iso-8859-1")
        r.insert(0, "iso-8859-1")
    elif r[0] == "cp1257" and "iso-8859-1" in top3:
        r.remove("iso-8859-1")
        r.insert(0, "iso-8859-1")

    return r


def fit_models(ext_rows, legacy_X, legacy_y, legacy_b):
    return lc.fit_route_models(ext_rows, legacy_X, legacy_y, legacy_b)


def eval_one(models, test_rows):
    base_hits = Counter()
    rr_hits = Counter()
    n = 0
    changes = 0
    beneficial = 0
    harmful = 0
    changed_pairs = Counter()

    for s in test_rows:
        model = models.get(s["route"])
        if model is None or s["label"] not in model[1].classes_:
            continue
        rank = base.rank_model(model, base.scorer_features(s["data"]))
        rr = rerank(s["data"], rank)
        truth = s["label"]
        n += 1

        for k in (1, 3, 5):
            base_hits[k] += int(truth in rank[:k])
            rr_hits[k] += int(truth in rr[:k])

        if rank and rr and rank[0] != rr[0]:
            changes += 1
            changed_pairs[(rank[0], rr[0])] += 1
            before = int(rank[0] == truth)
            after = int(rr[0] == truth)
            if after > before:
                beneficial += 1
            elif after < before:
                harmful += 1

    def metrics(h):
        return {
            "n": n,
            "top1": h[1] / max(1, n),
            "top3": h[3] / max(1, n),
            "top5": h[5] / max(1, n),
            "score": (4*h[1] + 2*h[3] + h[5]) / max(1, n),
        }

    return {
        "base": metrics(base_hits),
        "reranked": metrics(rr_hits),
        "changes": changes,
        "beneficial_changes": beneficial,
        "harmful_changes": harmful,
        "changed_pairs": [
            {"from": a, "to": b, "n": c}
            for (a,b), c in changed_pairs.most_common()
        ],
    }


def main():
    external, _, stats = base.collect_external()
    legacy_X, legacy_y, legacy_b = base.load_legacy_training()

    used_paths = sorted(set(s["warc_path"] for s in external))
    fold_assign = {
        p: int.from_bytes(hashlib.sha256(("learning-curve:" + p).encode()).digest()[:8], "big") % FOLDS
        for p in used_paths
    }

    fold_results = []
    total = {
        "base": Counter(),
        "reranked": Counter(),
        "n": 0,
        "changes": 0,
        "beneficial": 0,
        "harmful": 0,
    }

    for fold in range(FOLDS):
        train_pool = [s for s in external if fold_assign[s["warc_path"]] != fold]
        test_rows = [s for s in external if fold_assign[s["warc_path"]] == fold]
        ext_train = lc.deterministic_nested_subset(train_pool, TRAIN_N)
        models = fit_models(ext_train, legacy_X, legacy_y, legacy_b)
        res = eval_one(models, test_rows)
        fold_results.append({
            "fold": fold,
            "actual_external_train_n": len(ext_train),
            "test_n_raw": len(test_rows),
            "result": res,
        })

        n = res["base"]["n"]
        total["n"] += n
        for m in ("top1","top3","top5","score"):
            total["base"][m] += res["base"][m] * n
            total["reranked"][m] += res["reranked"][m] * n
        total["changes"] += res["changes"]
        total["beneficial"] += res["beneficial_changes"]
        total["harmful"] += res["harmful_changes"]

    n = max(1, total["n"])
    pooled_base = {m: total["base"][m]/n for m in ("top1","top3","top5","score")}
    pooled_rr = {m: total["reranked"][m]/n for m in ("top1","top3","top5","score")}

    print(json.dumps({
        "phase": "heldout_minimal_reranker_ab",
        "external_n": len(external),
        "warc_paths_used_n": len(used_paths),
        "selector": {"S1":"utf8 validity","S2":"NUL presence","S3":base.S3},
        "rules": [
            "UTF-8 BOM -> prefer utf-8-sig if already ranked",
            "strict UTF-8 valid non-BOM -> prefer utf-8 if already ranked",
            "ASCII-only -> prefer ascii if already ranked",
            "small within-top3 iso-8859-1 calibration against iso-8859-9/cp1257",
        ],
        "pooled": {
            "base": pooled_base,
            "reranked": pooled_rr,
            "delta": {m: pooled_rr[m]-pooled_base[m] for m in pooled_base},
            "changes": total["changes"],
            "beneficial_changes": total["beneficial"],
            "harmful_changes": total["harmful"],
        },
        "folds": fold_results,
        "collection_stats": dict(stats),
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
