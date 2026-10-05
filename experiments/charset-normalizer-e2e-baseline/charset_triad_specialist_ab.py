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
import charset_candidate_calibration_ab as calmod

OUTER_FOLDS = 4
INNER_FOLDS = 2
TRAIN_N = 300
TRIAD = ("utf-8", "cp1251", "gb18030")


def inner_assign(path):
    return int.from_bytes(hashlib.sha256(("triad-cal:" + path).encode()).digest()[:8], "big") % INNER_FOLDS


def score_map(model_tuple, data):
    scaler, model = model_tuple
    x = base.scorer_features(data)
    scores = model.decision_function(scaler.transform(x[None,:]))
    if scores.ndim == 1:
        scores = np.column_stack([-scores, scores])
    return {c: float(s) for c,s in zip(model.classes_, scores[0])}


def triad_feature(data, route, smap):
    vals = [smap.get(c, -20.0) for c in TRIAD]
    margins = [
        vals[0]-vals[1], vals[0]-vals[2], vals[1]-vals[2],
        max(vals)-sorted(vals)[-2] if len(vals) >= 2 else 0.0,
    ]
    b = np.frombuffer(data[:4096], dtype=np.uint8)
    scalars = [
        float(rr.strict_utf8(data)),
        float(rr.has_utf8_bom(data)),
        float(rr.ascii_only(data)),
        float(np.mean(b >= 128)) if len(b) else 0.0,
        float(np.mean(b == 0)) if len(b) else 0.0,
        float(len(b)),
    ]
    route_oh = [1.0 if route == r else 0.0 for r in ("U","N","RL","RH")]
    return np.asarray(vals + margins + scalars + route_oh, dtype=np.float32)


def make_hybrid(model, cal, s, classes, families):
    rank, scores = calmod.raw_rank_scores(model, s["data"])
    rule_rank = rr.rerank(s["data"], rank)
    cal_rank = calmod.choose_with_calibrator(
        cal, s["data"], s["route"], rank, scores, classes, families
    )
    hybrid = rule_rank if (rule_rank and rank and rule_rank[0] != rank[0]) else cal_rank
    return rank, hybrid


def crossfit_triad_rows(train_pool, legacy_X, legacy_y, legacy_b, classes, families):
    Xrows=[]; yrows=[]
    for inner in range(INNER_FOLDS):
        itr=[s for s in train_pool if inner_assign(s["warc_path"]) != inner]
        iva=[s for s in train_pool if inner_assign(s["warc_path"]) == inner]
        itr=lc.deterministic_nested_subset(itr, TRAIN_N)
        models=lc.fit_route_models(itr, legacy_X, legacy_y, legacy_b)
        cal=calmod.crossfit_calibration_rows(itr, legacy_X, legacy_y, legacy_b, classes, families)

        for s in iva:
            if s["label"] not in TRIAD:
                continue
            model=models.get(s["route"])
            if model is None or s["label"] not in model[1].classes_:
                continue
            rank, hybrid=make_hybrid(model,cal,s,classes,families)
            if not hybrid or hybrid[0] not in TRIAD:
                continue
            smap=score_map(model,s["data"])
            Xrows.append(triad_feature(s["data"],s["route"],smap))
            yrows.append(s["label"])

    if len(set(yrows)) < 2:
        return None
    X=np.stack(Xrows); y=np.asarray(yrows)
    sc=StandardScaler(); Xs=sc.fit_transform(X)
    clf=LogisticRegression(max_iter=2000,C=1.0,solver="lbfgs",class_weight="balanced")
    clf.fit(Xs,y)
    return sc,clf,len(yrows),Counter(yrows)


def apply_triad(triad_cal, model, s, hybrid):
    if triad_cal is None or not hybrid or hybrid[0] not in TRIAD:
        return hybrid
    # Only intervene if at least two triad labels are already in scorer top-3.
    rank,_=calmod.raw_rank_scores(model,s["data"])
    if sum(c in TRIAD for c in rank[:3]) < 2:
        return hybrid
    sc,clf,_,_=triad_cal
    smap=score_map(model,s["data"])
    x=triad_feature(s["data"],s["route"],smap)
    pred=clf.predict(sc.transform(x[None,:]))[0]
    if pred not in rank[:3]:
        return hybrid
    out=list(hybrid)
    if pred in out:
        out.remove(pred)
        out.insert(0,pred)
    return out


def main():
    external,_,stats=base.collect_external()
    legacy_X,legacy_y,legacy_b=base.load_legacy_training()
    classes,families=calmod.build_vocab(legacy_y)
    paths=sorted(set(s["warc_path"] for s in external))
    fold_assign={
        p:int.from_bytes(hashlib.sha256(("learning-curve:"+p).encode()).digest()[:8],"big")%OUTER_FOLDS
        for p in paths
    }

    folds=[]; total=Counter()
    for fold in range(OUTER_FOLDS):
        train_pool=[s for s in external if fold_assign[s["warc_path"]] != fold]
        test_rows=[s for s in external if fold_assign[s["warc_path"]] == fold]
        ext_train=lc.deterministic_nested_subset(train_pool,TRAIN_N)
        models=lc.fit_route_models(ext_train,legacy_X,legacy_y,legacy_b)
        cal=calmod.crossfit_calibration_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)
        triad_cal=crossfit_triad_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)

        n=0; hhit=0; thit=0; changes=0; ben=0; harm=0
        for s in test_rows:
            model=models.get(s["route"])
            if model is None or s["label"] not in model[1].classes_:
                continue
            _,hybrid=make_hybrid(model,cal,s,classes,families)
            tri=apply_triad(triad_cal,model,s,hybrid)
            truth=s["label"]
            n+=1
            h=int(hybrid and hybrid[0]==truth)
            t=int(tri and tri[0]==truth)
            hhit+=h; thit+=t
            if hybrid and tri and hybrid[0]!=tri[0]:
                changes+=1
                if t>h: ben+=1
                elif t<h: harm+=1

        row={
            "fold":fold,"n":n,
            "hybrid_top1":hhit/max(1,n),
            "triad_top1":thit/max(1,n),
            "changes":changes,"beneficial":ben,"harmful":harm,
            "triad_training_rows":triad_cal[2] if triad_cal else 0,
            "triad_training_counts":dict(triad_cal[3]) if triad_cal else {},
        }
        folds.append(row)
        total["n"]+=n; total["hhit"]+=hhit; total["thit"]+=thit
        total["changes"]+=changes; total["beneficial"]+=ben; total["harmful"]+=harm

    pooled={
        "n":total["n"],
        "hybrid_top1":total["hhit"]/max(1,total["n"]),
        "triad_top1":total["thit"]/max(1,total["n"]),
        "delta":(total["thit"]-total["hhit"])/max(1,total["n"]),
        "changes":total["changes"],
        "beneficial":total["beneficial"],
        "harmful":total["harmful"],
    }

    print(json.dumps({
        "phase":"triad_specialist_ab",
        "triad":TRIAD,
        "pooled":pooled,
        "folds":folds,
        "collection_stats":dict(stats),
        "decision_rule":"Keep only if pooled improves and at least 3/4 folds are non-negative versus hybrid."
    },indent=2,sort_keys=True))


if __name__=="__main__":
    main()
