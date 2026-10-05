from __future__ import annotations

import hashlib
import json
from collections import Counter

import numpy as np
from sklearn.linear_model import LogisticRegression
from sklearn.preprocessing import StandardScaler

import charset_external_scorer_learning_curve as lc
import charset_external_scorer_domain_shift_ab as base
import charset_minimal_reranker_ab as rr

OUTER_FOLDS = 4
INNER_FOLDS = 2
TRAIN_N = 300
TOP_CANDIDATES = 3
ROUTES = ("U", "N", "RL", "RH")


def raw_rank_scores(model_tuple, data):
    scaler, model = model_tuple
    x = base.scorer_features(data)
    scores = model.decision_function(scaler.transform(x[None, :]))
    if scores.ndim == 1:
        scores = np.column_stack([-scores, scores])
    scores = scores[0]
    order = np.argsort(scores)[::-1]
    return list(model.classes_[order]), scores[order]


def fam(x):
    if x.startswith("utf-"):
        return "utf"
    if x.startswith("cp12"):
        return "singlebyte-win"
    if x.startswith("iso-8859") or x.startswith("iso8859"):
        return "singlebyte-iso"
    if x in ("shift-jis","euc-jp","euc-kr","big5","gb18030","gbk","cp949"):
        return "cjk"
    if x.startswith("koi8"):
        return "koi8"
    if x == "ascii":
        return "ascii"
    return "other"


def build_vocab(legacy_y):
    classes = sorted(set(legacy_y.tolist()))
    families = ["ascii","cjk","koi8","other","singlebyte-iso","singlebyte-win","utf"]
    return classes, families


def candidate_feature(data, route, candidate, rank_idx, score, top_score, second_score, classes, families):
    class_idx = {c:i for i,c in enumerate(classes)}
    fam_idx = {c:i for i,c in enumerate(families)}

    vec = [
        float(score),
        float(score - top_score),
        float(score - second_score),
        float(rank_idx),
        float(rr.has_utf8_bom(data)),
        float(rr.strict_utf8(data)),
        float(rr.ascii_only(data)),
    ]
    route_oh = [1.0 if route == r else 0.0 for r in ROUTES]
    cand_oh = [0.0] * len(classes)
    if candidate in class_idx:
        cand_oh[class_idx[candidate]] = 1.0
    f = fam(candidate)
    fam_oh = [0.0] * len(families)
    if f in fam_idx:
        fam_oh[fam_idx[f]] = 1.0
    return np.asarray(vec + route_oh + cand_oh + fam_oh, dtype=np.float32)


def inner_assign(path):
    return int.from_bytes(hashlib.sha256(("candidate-cal:" + path).encode()).digest()[:8], "big") % INNER_FOLDS


def crossfit_calibration_rows(train_pool, legacy_X, legacy_y, legacy_b, classes, families):
    Xrows = []
    yrows = []

    for inner in range(INNER_FOLDS):
        inner_train = [s for s in train_pool if inner_assign(s["warc_path"]) != inner]
        inner_val = [s for s in train_pool if inner_assign(s["warc_path"]) == inner]
        inner_train = lc.deterministic_nested_subset(inner_train, TRAIN_N)
        models = lc.fit_route_models(inner_train, legacy_X, legacy_y, legacy_b)

        for s in inner_val:
            model = models.get(s["route"])
            if model is None or s["label"] not in model[1].classes_:
                continue
            rank, scores = raw_rank_scores(model, s["data"])
            if len(rank) < 2:
                continue
            top_score = float(scores[0])
            second_score = float(scores[1])
            for i, cand in enumerate(rank[:TOP_CANDIDATES]):
                Xrows.append(candidate_feature(
                    s["data"], s["route"], cand, i,
                    float(scores[i]), top_score, second_score,
                    classes, families
                ))
                yrows.append(int(cand == s["label"]))

    if not Xrows:
        return None
    X = np.stack(Xrows)
    y = np.asarray(yrows, dtype=np.int32)
    scaler = StandardScaler()
    Xs = scaler.fit_transform(X)
    model = LogisticRegression(
        max_iter=2000,
        C=1.0,
        class_weight="balanced",
        solver="lbfgs",
    )
    model.fit(Xs, y)
    return scaler, model, len(yrows), int(y.sum())


def choose_with_calibrator(cal, data, route, rank, scores, classes, families):
    scaler, model, _, _ = cal
    if len(rank) < 2:
        return rank
    top_score = float(scores[0])
    second_score = float(scores[1])
    cand = rank[:TOP_CANDIDATES]
    X = np.stack([
        candidate_feature(
            data, route, c, i, float(scores[i]),
            top_score, second_score, classes, families
        )
        for i,c in enumerate(cand)
    ])
    p = model.predict_proba(scaler.transform(X))[:,1]
    winner = int(np.argmax(p))
    if winner == 0:
        return rank
    out = list(rank)
    chosen = out.pop(winner)
    out.insert(0, chosen)
    return out


def eval_outer(train_pool, test_rows, legacy_X, legacy_y, legacy_b, classes, families):
    ext_train = lc.deterministic_nested_subset(train_pool, TRAIN_N)
    models = lc.fit_route_models(ext_train, legacy_X, legacy_y, legacy_b)
    cal = crossfit_calibration_rows(train_pool, legacy_X, legacy_y, legacy_b, classes, families)

    hits = {"base":0, "rule":0, "cal":0}
    n = 0
    changes = Counter()
    beneficial = Counter()
    harmful = Counter()

    for s in test_rows:
        model = models.get(s["route"])
        if model is None or s["label"] not in model[1].classes_:
            continue
        rank, scores = raw_rank_scores(model, s["data"])
        rule_rank = rr.rerank(s["data"], rank)
        cal_rank = choose_with_calibrator(cal, s["data"], s["route"], rank, scores, classes, families)
        truth = s["label"]
        n += 1
        outs = {"base": rank, "rule": rule_rank, "cal": cal_rank}
        for name, r in outs.items():
            hits[name] += int(r and r[0] == truth)
            if name != "base" and r and rank and r[0] != rank[0]:
                changes[name] += 1
                before = int(rank[0] == truth)
                after = int(r[0] == truth)
                if after > before:
                    beneficial[name] += 1
                elif after < before:
                    harmful[name] += 1

    return {
        "n": n,
        "actual_external_train_n": len(ext_train),
        "calibration_rows": cal[2] if cal else 0,
        "calibration_positive_rows": cal[3] if cal else 0,
        "base_top1": hits["base"]/max(1,n),
        "rule_top1": hits["rule"]/max(1,n),
        "cal_top1": hits["cal"]/max(1,n),
        "rule_changes": changes["rule"],
        "rule_beneficial": beneficial["rule"],
        "rule_harmful": harmful["rule"],
        "cal_changes": changes["cal"],
        "cal_beneficial": beneficial["cal"],
        "cal_harmful": harmful["cal"],
    }


def main():
    external, _, stats = base.collect_external()
    legacy_X, legacy_y, legacy_b = base.load_legacy_training()
    classes, families = build_vocab(legacy_y)

    used_paths = sorted(set(s["warc_path"] for s in external))
    fold_assign = {
        p: int.from_bytes(hashlib.sha256(("learning-curve:" + p).encode()).digest()[:8], "big") % OUTER_FOLDS
        for p in used_paths
    }

    folds = []
    sums = Counter()
    total_n = 0
    for fold in range(OUTER_FOLDS):
        train_pool = [s for s in external if fold_assign[s["warc_path"]] != fold]
        test_rows = [s for s in external if fold_assign[s["warc_path"]] == fold]
        res = eval_outer(train_pool, test_rows, legacy_X, legacy_y, legacy_b, classes, families)
        folds.append({"fold":fold, **res})
        n = res["n"]
        total_n += n
        for name in ("base_top1","rule_top1","cal_top1"):
            sums[name] += res[name]*n
        for name in (
            "rule_changes","rule_beneficial","rule_harmful",
            "cal_changes","cal_beneficial","cal_harmful"
        ):
            sums[name] += res[name]

    pooled = {
        "n": total_n,
        "base_top1": sums["base_top1"]/max(1,total_n),
        "rule_top1": sums["rule_top1"]/max(1,total_n),
        "cal_top1": sums["cal_top1"]/max(1,total_n),
        "rule_changes": sums["rule_changes"],
        "rule_beneficial": sums["rule_beneficial"],
        "rule_harmful": sums["rule_harmful"],
        "cal_changes": sums["cal_changes"],
        "cal_beneficial": sums["cal_beneficial"],
        "cal_harmful": sums["cal_harmful"],
    }
    pooled["delta_cal_vs_base"] = pooled["cal_top1"] - pooled["base_top1"]
    pooled["delta_cal_vs_rule"] = pooled["cal_top1"] - pooled["rule_top1"]

    print(json.dumps({
        "phase":"candidate_level_calibration_ab",
        "external_n":len(external),
        "warc_paths_used_n":len(used_paths),
        "train_n_request":TRAIN_N,
        "top_candidates":TOP_CANDIDATES,
        "pooled":pooled,
        "folds":folds,
        "collection_stats":dict(stats),
        "decision_rule":"Keep candidate calibrator only if pooled gain beats rule reranker and at least 3/4 outer folds are non-negative versus rule reranker."
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
