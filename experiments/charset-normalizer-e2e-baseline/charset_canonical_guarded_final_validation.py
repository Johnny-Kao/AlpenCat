from __future__ import annotations
import hashlib,json
from collections import Counter

import charset_external_scorer_learning_curve as lc
import charset_external_scorer_domain_shift_ab as base
import charset_candidate_calibration_ab as calmod
import charset_triad_specialist_ab as tri
import charset_post_gate_050_error_decomposition as gate050
import charset_pair_specialist_ab as pairmod

OUTER_FOLDS=4
TRAIN_N=300
SIG_PAIR=("utf-8","utf-8-sig")
GB_PAIR=("utf-8","gb18030")
RATE_THRESHOLD=0.02

def fit_one(pair,*args):
    old=pairmod.PAIR; pairmod.PAIR=pair
    try: return pairmod.fit_pair(*args)
    finally: pairmod.PAIR=old

def apply_one(pair_cal,pair,model,s,baseline):
    old=pairmod.PAIR; pairmod.PAIR=pair
    try: return pairmod.apply_pair(pair_cal,model,s,baseline)
    finally: pairmod.PAIR=old

def replacement_rate(data):
    if not data: return 0.0
    txt=data.decode("utf-8",errors="replace")
    return txt.count("\ufffd")/max(1,len(txt))

def apply_guarded_gb(gb_cal,model,s,baseline):
    out=apply_one(gb_cal,GB_PAIR,model,s,baseline)
    if not out or not baseline or out[0]==baseline[0]:
        return out
    if baseline[0]=="gb18030" and out[0]=="utf-8" and replacement_rate(s["data"])>RATE_THRESHOLD:
        return baseline
    return out

def main():
    external,_,stats=base.collect_external()
    fp=hashlib.sha256()
    for s in sorted(external,key=lambda x:(x["warc_path"],x.get("host",""),x["label"],len(x["data"]))):
        fp.update(s["warc_path"].encode()); fp.update(b"\0")
        fp.update(s.get("host","").encode()); fp.update(b"\0")
        fp.update(s["label"].encode()); fp.update(b"\0")
        fp.update(hashlib.sha256(s["data"]).digest())

    legacy_X,legacy_y,legacy_b=base.load_legacy_training()
    classes,families=calmod.build_vocab(legacy_y)
    paths=sorted(set(s["warc_path"] for s in external))
    fold_assign={p:int.from_bytes(hashlib.sha256(("learning-curve:"+p).encode()).digest()[:8],"big")%OUTER_FOLDS for p in paths}

    total=Counter(); folds=[]
    for fold in range(OUTER_FOLDS):
        train_pool=[s for s in external if fold_assign[s["warc_path"]] != fold]
        test_rows=[s for s in external if fold_assign[s["warc_path"]] == fold]
        ext_train=lc.deterministic_nested_subset(train_pool,TRAIN_N)
        models=lc.fit_route_models(ext_train,legacy_X,legacy_y,legacy_b)
        cal=calmod.crossfit_calibration_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)
        triad_cal=tri.crossfit_triad_rows(train_pool,legacy_X,legacy_y,legacy_b,classes,families)
        sig_cal=fit_one(SIG_PAIR,train_pool,legacy_X,legacy_y,legacy_b,classes,families)
        gb_cal=fit_one(GB_PAIR,train_pool,legacy_X,legacy_y,legacy_b,classes,families)

        c=Counter()
        for s in test_rows:
            model=models.get(s["route"])
            if model is None or s["label"] not in model[1].classes_: continue
            _,hybrid=tri.make_hybrid(model,cal,s,classes,families)
            b=gate050.apply_gated(triad_cal,model,s,hybrid)
            sig=apply_one(sig_cal,SIG_PAIR,model,s,b)
            out=apply_guarded_gb(gb_cal,model,s,sig)
            truth=s["label"]
            c["n"]+=1
            c["sig_hits"]+=int(sig and sig[0]==truth)
            c["final_hits"]+=int(out and out[0]==truth)
            if out and sig and out[0]!=sig[0]:
                c["changes"]+=1
                before=int(sig[0]==truth); after=int(out[0]==truth)
                if after>before: c["beneficial"]+=1
                elif after<before: c["harmful"]+=1

        folds.append({
            "fold":fold,"n":c["n"],
            "sig_top1":c["sig_hits"]/max(1,c["n"]),
            "final_top1":c["final_hits"]/max(1,c["n"]),
            "changes":c["changes"],"beneficial":c["beneficial"],"harmful":c["harmful"]
        })
        for k,v in c.items(): total[k]+=v

    print(json.dumps({
      "phase":"canonical_guarded_final_validation",
      "corpus_fingerprint":fp.hexdigest(),
      "replacement_rate_threshold":RATE_THRESHOLD,
      "external_n":len(external),
      "pooled":{
        "n":total["n"],
        "sig_top1":total["sig_hits"]/total["n"],
        "final_top1":total["final_hits"]/total["n"],
        "hits":total["final_hits"],
        "changes":total["changes"],
        "beneficial":total["beneficial"],
        "harmful":total["harmful"]
      },
      "folds":folds,
      "target_charset_normalizer":0.8714285714285714,
      "collection_stats":dict(stats)
    },indent=2,sort_keys=True))

if __name__=="__main__": main()
