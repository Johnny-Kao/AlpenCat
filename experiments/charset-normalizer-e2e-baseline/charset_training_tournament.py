from __future__ import annotations

import json
import math
import sys
import time
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

import numpy as np
from sklearn.linear_model import LogisticRegression
from sklearn.preprocessing import StandardScaler

sys.path.append(str(Path(__file__).parent))

from charset_cost_aware_routing_tree import (
    collect,
    feature_vector as route_feature_vector,
    fold_of,
)

FOLDS=3
TOPK=(1,3,5)
MAX_DEPTH=4
MAX_LEAVES=6


def hmt768(data: bytes) -> bytes:
    if len(data)<=768:
        return data
    m=len(data)//2
    return data[:256]+data[m-128:m+128]+data[-256:]


def scorer_features(data: bytes) -> np.ndarray:
    data=hmt768(data)
    a=np.frombuffer(data,dtype=np.uint8)
    n=max(1,len(a))
    unigram=np.bincount(a,minlength=256).astype(np.float32)/n
    bigram=np.zeros(256,dtype=np.float32)
    if len(a)>=2:
        bins=((a[:-1].astype(np.uint16)*257+a[1:].astype(np.uint16))&255).astype(np.int32)
        bigram=np.bincount(bins,minlength=256).astype(np.float32)/(len(a)-1)
    scalars=np.array([
        math.log2(n+1)/16.0,
        float(np.mean(a>=128)) if len(a) else 0.0,
        float(np.mean(a==0)) if len(a) else 0.0,
        float(np.mean((a>=32)&(a<=126))) if len(a) else 0.0,
        float(np.mean(a==10)) if len(a) else 0.0,
        float(np.mean(a==13)) if len(a) else 0.0,
    ],dtype=np.float32)
    return np.concatenate([unigram,bigram,scalars])


def fit_linear(X,y,C=4.0):
    scaler=StandardScaler()
    Xs=scaler.fit_transform(X)
    model=LogisticRegression(max_iter=2500,solver="lbfgs",C=C)
    model.fit(Xs,y)
    return scaler,model


def rank_model(model_tuple,x):
    scaler,model=model_tuple
    scores=model.decision_function(scaler.transform(x[None,:]))
    if scores.ndim==1:
        scores=np.column_stack([-scores,scores])
    return list(model.classes_[np.argsort(scores[0])[::-1]])


def eval_global(rows,score_X,y,groups):
    hits={k:0 for k in TOPK}
    total=0
    for fold in range(FOLDS):
        tr=groups!=fold; te=groups==fold
        m=fit_linear(score_X[tr],y[tr])
        for gi in np.where(te)[0]:
            rank=rank_model(m,score_X[gi])
            for k in TOPK:
                hits[k]+=int(y[gi] in rank[:k])
            total+=1
    return {f"top{k}":hits[k]/total for k in TOPK}


@dataclass
class SplitNode:
    idx: np.ndarray
    depth:int
    feature:int|None=None
    threshold:float|None=None
    left:"SplitNode|None"=None
    right:"SplitNode|None"=None
    leaf_id:int|None=None


def candidate_thresholds(col):
    vals=np.unique(col)
    if len(vals)<=1:
        return []
    if len(vals)<=10:
        return [(float(a)+float(b))/2 for a,b in zip(vals[:-1],vals[1:])]
    return [float(x) for x in np.unique(np.quantile(col,[.15,.3,.5,.7,.85]))]


LOSS_CACHE={}
FIT_CALLS=0
LOSS_CACHE_HITS=0
EXACT_SPLITS_EVALUATED=0
PROXY_TOPK=4


def label_entropy(labels):
    counts=np.array(list(Counter(labels).values()),dtype=np.float64)
    if counts.size==0:
        return 0.0
    p=counts/counts.sum()
    return float(-(p*np.log2(p)).sum())


def partition_key(idxs,C):
    # Stable compact key. The outer fold is already encoded by the index set.
    a=np.asarray(idxs,dtype=np.int32)
    return (float(C), a.tobytes())


def proxy_split_value(y,parent_idx,li,ri,feature_cost,lambda_cost):
    # Cheap shortlist only. Exact acceptance still uses downstream scorer loss.
    ph=label_entropy(y[parent_idx])
    wh=(len(li)*label_entropy(y[li])+len(ri)*label_entropy(y[ri]))/len(parent_idx)
    balance=min(len(li),len(ri))/len(parent_idx)
    return (ph-wh)*(0.5+balance)-lambda_cost*feature_cost


def child_loss(score_X,y,idxs,C=4.0):
    global FIT_CALLS, LOSS_CACHE_HITS
    if len(idxs)<20 or len(np.unique(y[idxs]))<2:
        return 1.0
    key=partition_key(idxs,C)
    if key in LOSS_CACHE:
        LOSS_CACHE_HITS+=1
        return LOSS_CACHE[key]

    mask=np.array([(int(i)*2654435761)%4==0 for i in idxs])
    if mask.sum()<3 or (~mask).sum()<10:
        LOSS_CACHE[key]=1.0
        return 1.0

    tr=idxs[~mask]; va=idxs[mask]
    try:
        FIT_CALLS+=1
        m=fit_linear(score_X[tr],y[tr],C=C)
    except Exception:
        LOSS_CACHE[key]=1.0
        return 1.0

    correct=0
    for gi in va:
        rank=rank_model(m,score_X[gi])
        correct+=int(y[gi] in rank[:1])
    loss=1.0-correct/len(va)
    LOSS_CACHE[key]=loss
    return loss


def train_lossaware_tree(route_X,score_X,y,train_idx,feature_costs,lambda_cost=0.0,C=4.0,max_leaves=6):
    global EXACT_SPLITS_EVALUATED
    root=SplitNode(train_idx.copy(),0)
    leaves=[root]

    while len(leaves)<max_leaves:
        best=None

        for leaf in leaves:
            if leaf.depth>=MAX_DEPTH or len(leaf.idx)<40:
                continue

            # Phase 1: very cheap scan over every candidate threshold.
            proxy_candidates=[]
            for j in range(route_X.shape[1]):
                col=route_X[leaf.idx,j]
                for t in candidate_thresholds(col):
                    lm=col<=t
                    if lm.sum()<20 or (~lm).sum()<20:
                        continue
                    li=leaf.idx[lm]; ri=leaf.idx[~lm]
                    proxy=proxy_split_value(
                        y,leaf.idx,li,ri,feature_costs[j],lambda_cost
                    )
                    proxy_candidates.append((proxy,j,t,li,ri))

            if not proxy_candidates:
                continue

            # Phase 2: only the best few cheap candidates buy real scorer fits.
            proxy_candidates.sort(key=lambda x:x[0],reverse=True)
            p_loss=child_loss(score_X,y,leaf.idx,C=C)
            for _,j,t,li,ri in proxy_candidates[:PROXY_TOPK]:
                EXACT_SPLITS_EVALUATED+=1
                l_loss=child_loss(score_X,y,li,C=C)
                r_loss=child_loss(score_X,y,ri,C=C)
                w_loss=(len(li)*l_loss+len(ri)*r_loss)/len(leaf.idx)
                gain=p_loss-w_loss
                value=gain-lambda_cost*feature_costs[j]
                cand=(value,gain,-feature_costs[j],leaf,j,t,li,ri)
                if best is None or cand[:3]>best[:3]:
                    best=cand

        if best is None or best[0]<=0:
            break

        _,_,_,leaf,j,t,li,ri=best
        leaf.feature=j; leaf.threshold=t
        leaf.left=SplitNode(li,leaf.depth+1)
        leaf.right=SplitNode(ri,leaf.depth+1)
        leaves=[x for x in leaves if x is not leaf]+[leaf.left,leaf.right]

    for lid,leaf in enumerate(leaves):
        leaf.leaf_id=lid
    return root,leaves


def route(root,x):
    node=root; depth=0
    while node.feature is not None:
        depth+=1
        node=node.left if x[node.feature]<=node.threshold else node.right
    return node.leaf_id,depth


def eval_routed(rows,route_X,score_X,y,groups,feature_costs,lambda_cost=0.0,C=4.0,max_leaves=6):
    hits={k:0 for k in TOPK}
    total=0
    depths=[]
    leaf_sizes=[]
    leaf_classes=[]
    for fold in range(FOLDS):
        tr=np.where(groups!=fold)[0]
        te=np.where(groups==fold)[0]
        root,leaves=train_lossaware_tree(route_X,score_X,y,tr,feature_costs,lambda_cost=lambda_cost,C=C,max_leaves=max_leaves)

        leaf_train={}
        for gi in tr:
            lid,_=route(root,route_X[gi])
            leaf_train.setdefault(lid,[]).append(gi)

        models={}
        global_model=fit_linear(score_X[tr],y[tr],C=C)
        for lid,idxs in leaf_train.items():
            idxs=np.array(idxs,dtype=int)
            leaf_sizes.append(len(idxs)); leaf_classes.append(len(np.unique(y[idxs])))
            if len(idxs)>=20 and len(np.unique(y[idxs]))>=2:
                try:
                    models[lid]=fit_linear(score_X[idxs],y[idxs],C=C)
                except Exception:
                    pass

        for gi in te:
            lid,d=route(root,route_X[gi]); depths.append(d)
            rank=rank_model(models.get(lid,global_model),score_X[gi])
            for k in TOPK:
                hits[k]+=int(y[gi] in rank[:k])
            total+=1

    return {
        **{f"top{k}":hits[k]/total for k in TOPK},
        "mean_depth":float(np.mean(depths)) if depths else 0.0,
        "p95_depth":float(np.quantile(depths,.95)) if depths else 0.0,
        "median_leaf_size":float(np.median(leaf_sizes)) if leaf_sizes else 0.0,
        "median_leaf_classes":float(np.median(leaf_classes)) if leaf_classes else 0.0,
    }


def round2_configs(method_id):
    configs={
        "D":[
            {"lambda_cost":0.0,"C":0.5,"max_leaves":5},
            {"lambda_cost":0.001,"C":1.0,"max_leaves":5},
            {"lambda_cost":0.003,"C":2.0,"max_leaves":6},
        ],
        "A":[
            {"lambda_cost":0.0,"C":1.0,"max_leaves":4},
            {"lambda_cost":0.0,"C":2.0,"max_leaves":5},
            {"lambda_cost":0.0,"C":4.0,"max_leaves":6},
        ],
        "B":[
            {"lambda_cost":0.001,"C":2.0,"max_leaves":5},
            {"lambda_cost":0.003,"C":2.0,"max_leaves":5},
            {"lambda_cost":0.01,"C":2.0,"max_leaves":6},
        ],
    }
    return configs[method_id]


def score_candidate(metrics,baseline):
    # Accuracy dominates. Small penalties discourage needless routing depth.
    return (
        4.0*(metrics["top1"]-baseline["top1"])
        +2.0*(metrics["top3"]-baseline["top3"])
        +(metrics["top5"]-baseline["top5"])
        -0.002*metrics["mean_depth"]
    )


def main():
    global FIT_CALLS, LOSS_CACHE_HITS, EXACT_SPLITS_EVALUATED
    t0=time.perf_counter()
    rows,_=collect()
    y=np.array([r["encoding"] for r in rows])
    groups=np.array([fold_of(r["path"].name) for r in rows])

    route_X=np.stack([route_feature_vector(r["path"]) for r in rows])
    score_X=np.stack([scorer_features(r["path"].read_bytes()) for r in rows])

    feature_costs=np.ones(route_X.shape[1],dtype=float)
    feature_costs[:5]=0.05

    baseline=eval_global(rows,score_X,y,groups)

    round2={}
    finalists=[]
    for method_id in ["D","A","B"]:
        variants=[]
        for variant_id,cfg in enumerate(round2_configs(method_id),start=1):
            metrics=eval_routed(rows,route_X,score_X,y,groups,feature_costs,**cfg)
            score=score_candidate(metrics,baseline)
            row={
                "method":method_id,
                "variant":variant_id,
                "config":cfg,
                "metrics":metrics,
                "score":score,
                "delta_vs_baseline":{
                    "top1":metrics["top1"]-baseline["top1"],
                    "top3":metrics["top3"]-baseline["top3"],
                    "top5":metrics["top5"]-baseline["top5"],
                },
            }
            variants.append(row)
        variants.sort(key=lambda x:x["score"],reverse=True)
        round2[method_id]=variants
        finalists.append(variants[0])

    finalists.sort(key=lambda x:x["score"],reverse=True)

    print(json.dumps({
        "phase":"round2_top3_x3",
        "files":len(rows),
        "encodings":len(set(y)),
        "folds":FOLDS,
        "baseline":baseline,
        "round2":round2,
        "finalists":finalists,
        "winner":finalists[0],
        "runner_up":finalists[1],
        "optimization_stats":{
            "proxy_topk":PROXY_TOPK,
            "exact_split_evaluations":EXACT_SPLITS_EVALUATED,
            "scorer_fit_calls":FIT_CALLS,
            "loss_cache_hits":LOSS_CACHE_HITS,
            "loss_cache_entries":len(LOSS_CACHE),
            "elapsed_seconds":time.perf_counter()-t0,
        },
    },indent=2,sort_keys=True))


if __name__=="__main__":
    main()
