from __future__ import annotations

import hashlib
import json
import os
from collections import Counter

import numpy as np
from sklearn.linear_model import LogisticRegression
from sklearn.preprocessing import StandardScaler

import charset_external_scorer_learning_curve as lc
import charset_external_scorer_domain_shift_ab as base
import charset_candidate_calibration_ab as calmod
import charset_triad_specialist_ab as tri
import charset_post_gate_050_error_decomposition as gate050
import charset_minimal_reranker_ab as rr

OUTER_FOLDS = 4
INNER_FOLDS = 2
TRAIN_N = 300
PAIR = (os.environ["PAIR_A"], os.environ["PAIR_B"])


def inner_assign(path):
    return int.from_bytes(hashlib.sha256(("pair-cal:" + "|".join(PAIR) + ":" + path).encode()).digest()[:8], "big") % INNER_FOLDS


def score_map(model_tuple, data):
    scaler, model = model_tuple
    x = base.scorer_features(data)
    scores = model.decision_function(scaler.transform(x[None,:]))
    if scores.ndim == 1:
        scores = np.column_stack([-scores, scores])
    return {c: float(s) for c,s in zip(model.classes_, scores[0])}


def feature(data, route, sm):
    a,b = PAIR
    sa,sb = sm.get(a,-20.0), sm.get(b,-20.0)
    arr = np.frombuffer(data[:4096], dtype=np.uint8)
    scalars = [
        sa, sb, sa-sb, abs(sa-sb),
        float(rr.strict_utf8(data)),
        float(rr.has_utf8_bom(data)),
        float(rr.ascii_only(data)),
        float(np.mean(arr >= 128)) if len(arr) else 0.0,
        float(np.mean(arr == 0)) if len(arr) else 0.0,
        float(len(arr)),
    ]
    route_oh = [1.0 if route == r else 0.0 for r in ("U","N","RL","RH")]
    return np.asarray(scalars + route_oh, dtype=np.float32)


def make_baseline(model, cal, triad_cal, s, classes, families):
    _, hybrid = tri.make_hybrid(model, cal, s, classes, families)
    return gate050.apply_gated(triad_cal, model, s, hybrid)


def fit_pair(train_pool, legacy_X, legacy_y, legacy_b, classes, families):
    Xrows=[]; yrows=[]
    for inner in range(INNER_FOLDS):
        itr=[s for s in train_pool if inner_assign(s["warc_path"]) != inner]
        iva=[s for s in train_pool if inner_assign(s["warc_path"]) == inner]
        itr=lc.deterministic_nested_subset(itr,TRAIN_N)
        models=lc.fit_route_models(itr,legacy_X,legacy_y,legacy_b)
        cal=calmod.crossfit_calibration_rows(itr,legacy_X,legacy_y,legacy_b,classes,families)
        triad_cal=tri.crossfit_triad_rows(itr,legacy_X,legacy_y,legacy_b,classes,families)

        for s in iva:
            if s["label"] not in PAIR:
                continue
            model=models.get(s["route"])
            if model is None or s["label"] not in model[1].classes_:
                continue
            rank,_=calmod.raw_rank_scores(model,s["data"])
            if not all(p in rank[:3] for p in PAIR):
                continue
            baseline=make_baseline(model,cal,triad_cal,s,classes,families)
            if not baseline or baseline[0] not in PAIR:
                continue
            Xrows.append(feature(s["data"],s["route"],score_map(model,s["data"])))
            yrows.append(s["label"])

    if len(set(yrows)) < 2:
        return None
    X=np.stack(Xrows); y=np.asarray(yrows)
    sc=StandardScaler(); Xs=sc.fit_transform(X)
    clf=LogisticRegression(max_iter=2000,C=1.0,solver="lbfgs",class_weight="balanced")
    clf.fit(Xs,y)
    return sc,clf,len(yrows),Counter(yrows)


def apply_pair(pair_cal, model, s, baseline):
    if pair_cal is None or not baseline or baseline[0] not in PAIR:
        return baseline
    rank,_=calmod.raw_rank_scores(model,s["data"])
    if not all(p in rank[:3] for p in PAIR):
        return baseline
    sc,clf,_,_=pair_cal
    pred=clf.predict(sc.transform(feature(s["data"],s["route"],score_map(model,s["data"]))[None,:]))[0]
    if pred not in rank[:3]:
        return baseline
    out=list(baseline)
    if pred in out:
        out.remove(pred); out.insert(0,pred)
    return out


def main():
    external,_,stats=base.collect_external()
    legacy_X,legacy_y,legacy_b=base.load_legacy_training()
    classes,families=calmod.build_vocab(legacy_y)
    paths=sorted(set(s["warc_path"] for s in external))
    fold_assign={p:int.from_bytes(hashlib.sha256(("learning-curve:"+p).encode()).digest()[:8],"big")%OUTER_FOLDS for p in paths}

    folds=[]; total=Counter()
    for fold in range(OUTER_FOLDS):
        train_pool=[s for s in external if fold_assign[s["warc_path"]] != fold]
        test_rows=[s for s in external if fold_assign[s["warc_path"]] == fold]
        ext_train=lc.deterministic_nested_subset(train_pool,TRAIN_N)
        models=lc.fit_route_models(ext_train,legacy_X,legacy_y,legacy_b)
        cal=calmod.crossfit_calibration_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)
        triad_cal=tri.crossfit_triad_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)
        pair_cal=fit_pair(train_pool,legacy_X,legacy_y,legacy_b,classes,families)

        n=bhit=phit=changes=ben=harm=0
        for s in test_rows:
            model=models.get(s["route"])
            if model is None or s["label"] not in model[1].classes_:
                continue
            baseline=make_baseline(model,cal,triad_cal,s,classes,families)
            out=apply_pair(pair_cal,model,s,baseline)
            truth=s["label"]
            n+=1
            b=int(baseline and baseline[0]==truth)
            p=int(out and out[0]==truth)
            bhit+=b; phit+=p
            if baseline and out and baseline[0]!=out[0]:
                changes+=1
                if p>b: ben+=1
                elif p<b: harm+=1

        row={
            "fold":fold,"n":n,
            "baseline_top1":bhit/max(1,n),
            "pair_top1":phit/max(1,n),
            "changes":changes,"beneficial":ben,"harmful":harm,
            "pair_training_rows":pair_cal[2] if pair_cal else 0,
            "pair_training_counts":dict(pair_cal[3]) if pair_cal else {},
        }
        folds.append(row)
        total["n"]+=n; total["bhit"]+=bhit; total["phit"]+=phit
        total["changes"]+=changes; total["beneficial"]+=ben; total["harmful"]+=harm

    pooled={
        "n":total["n"],
        "baseline_top1":total["bhit"]/max(1,total["n"]),
        "pair_top1":total["phit"]/max(1,total["n"]),
        "delta":(total["phit"]-total["bhit"])/max(1,total["n"]),
        "changes":total["changes"],"beneficial":total["beneficial"],"harmful":total["harmful"],
    }
    print(json.dumps({
        "phase":"pair_specialist_ab","pair":PAIR,"pooled":pooled,"folds":folds,
        "collection_stats":dict(stats)
    },indent=2,sort_keys=True))

if __name__=="__main__":
    main()
