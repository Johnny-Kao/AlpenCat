from __future__ import annotations
import hashlib, json
from collections import Counter

import charset_external_scorer_learning_curve as lc
import charset_external_scorer_domain_shift_ab as base
import charset_candidate_calibration_ab as calmod
import charset_triad_specialist_ab as tri

OUTER_FOLDS=4
TRAIN_N=300
CONF=0.50


def apply_gated(triad_cal, model, s, hybrid):
    if triad_cal is None or not hybrid or hybrid[0] not in tri.TRIAD:
        return hybrid
    rank,_=calmod.raw_rank_scores(model,s["data"])
    if sum(c in tri.TRIAD for c in rank[:3]) < 2:
        return hybrid
    sc,clf,_,_=triad_cal
    smap=tri.score_map(model,s["data"])
    x=tri.triad_feature(s["data"],s["route"],smap)
    probs=clf.predict_proba(sc.transform(x[None,:]))[0]
    idx=int(probs.argmax())
    pred=clf.classes_[idx]
    p=float(probs[idx])
    if p < CONF or pred not in rank[:3]:
        return hybrid
    out=list(hybrid)
    if pred in out:
        out.remove(pred); out.insert(0,pred)
    return out


def main():
    external,_,stats=base.collect_external()
    legacy_X,legacy_y,legacy_b=base.load_legacy_training()
    classes,families=calmod.build_vocab(legacy_y)
    paths=sorted(set(s["warc_path"] for s in external))
    fold_assign={p:int.from_bytes(hashlib.sha256(("learning-curve:"+p).encode()).digest()[:8],"big")%OUTER_FOLDS for p in paths}

    errors=Counter(); rank_hist=Counter(); by_route=Counter(); by_label=Counter()
    n=correct=0
    detailed=[]

    for fold in range(OUTER_FOLDS):
        train_pool=[s for s in external if fold_assign[s["warc_path"]] != fold]
        test_rows=[s for s in external if fold_assign[s["warc_path"]] == fold]
        ext_train=lc.deterministic_nested_subset(train_pool,TRAIN_N)
        models=lc.fit_route_models(ext_train,legacy_X,legacy_y,legacy_b)
        cal=calmod.crossfit_calibration_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)
        triad_cal=tri.crossfit_triad_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)

        for s in test_rows:
            model=models.get(s["route"])
            if model is None or s["label"] not in model[1].classes_:
                continue
            _,hybrid=tri.make_hybrid(model,cal,s,classes,families)
            out=apply_gated(triad_cal,model,s,hybrid)
            truth=s["label"]
            n+=1
            if out and out[0]==truth:
                correct+=1
                continue
            pred=out[0] if out else None
            errors[(truth,pred)] += 1
            by_route[s["route"]] += 1
            by_label[truth] += 1
            try:
                pos=out.index(truth)+1
            except Exception:
                pos=999
            rank_hist[min(pos,6)] += 1
            detailed.append({
                "fold":fold,"truth":truth,"pred":pred,"truth_rank":pos,
                "top5":out[:5] if out else [],
                "route":s["route"],"host":s.get("host",""),"warc_path":s["warc_path"]
            })

    print(json.dumps({
        "phase":"post_gate_050_error_decomposition",
        "evaluated_n":n,
        "top1":correct/max(1,n),
        "wrong_n":n-correct,
        "rank_histogram":{
            "rank2":rank_hist[2],"rank3":rank_hist[3],"rank4":rank_hist[4],
            "rank5":rank_hist[5],"not_top5":rank_hist[6],
        },
        "top_confusions":[{"truth":t,"pred":p,"n":c} for (t,p),c in errors.most_common(25)],
        "errors_by_route":dict(by_route),
        "errors_by_truth":dict(by_label),
        "errors":detailed,
        "collection_stats":dict(stats),
    },indent=2,sort_keys=True))

if __name__=="__main__":
    main()
