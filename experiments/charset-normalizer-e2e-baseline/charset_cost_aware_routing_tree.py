from __future__ import annotations

import hashlib
import json
import math
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

import numpy as np

ROOTS = [("chardet", Path("test-data")), ("char_dataset", Path("char-dataset"))]
FOLDS = 5
MAX_DEPTH = 4
MAX_LEAVES = 6
MIN_LEAF = 8

# Relative acquisition costs. These are deliberately coarse in v0:
# metadata/length are effectively free; one bounded byte scan can collect
# most byte counters together.
FEATURE_COST = {
    "known_markup_ext": 0.05,
    "known_record_ext": 0.05,
    "known_code_ext": 0.05,
    "known_text_ext": 0.05,
    "log_len": 0.05,
    "ascii_only_4k": 0.50,
    "utf8_valid_4k": 0.60,
    "high_byte_ratio": 1.00,
    "nul_ratio": 1.00,
    "lt_ratio": 1.00,
    "slash_ratio": 1.00,
    "quote_ratio": 1.00,
    "comma_ratio": 1.00,
    "newline_ratio": 1.00,
    "line_repeat_ratio": 1.00,
    "line_len_cv": 1.00,
}
FEATURES = list(FEATURE_COST)

MARKUP_EXT={".html",".htm",".xml",".xhtml",".svg"}
RECORD_EXT={".csv",".tsv",".srt",".vtt",".m3u",".m3u8"}
CODE_EXT={".json",".js",".css",".py",".hpp",".h",".c",".cpp"}
TEXT_EXT={".txt",".po",".md",".rst"}


def label_from_dir(root_name: str, path: Path) -> str | None:
    rel = path
    if root_name == "chardet":
        d = rel.parent.name
        if "-" not in d:
            return None
        enc, _lang = d.rsplit("-", 1)
        return None if enc == "None" else enc.lower().replace("_","-")
    d = rel.parent.name
    if d == "None":
        return None
    return d.lower().replace("_","-")


def canonical_encoding(label: str) -> str:
    aliases={
        "utf8":"utf-8","utf8-sig":"utf-8-sig","euc-jp":"euc-jp",
        "euc-kr":"euc-kr","shift-jis":"shift-jis","shift_jis":"shift-jis",
        "windows-1252":"cp1252","windows-1251":"cp1251","windows-1250":"cp1250",
        "windows-1255":"cp1255","windows-1256":"cp1256",
        "gbk":"gb18030",
    }
    return aliases.get(label,label)


def collect():
    by_hash={}
    source_counts=Counter()
    for root_name,root in ROOTS:
        # Corpus layout is one encoding directory below the repository root.
        # Deliberately do not recurse into .git or repository metadata.
        for d in root.iterdir():
            if not d.is_dir() or d.name.startswith("."):
                continue
            for p in d.iterdir():
                if not p.is_file() or p.name.startswith("."):
                    continue
                label=label_from_dir(root_name,p)
                if not label:
                    continue
                data=p.read_bytes()
                h=hashlib.sha256(data).hexdigest()
                label=canonical_encoding(label)
                source_counts[root_name]+=1
                if h not in by_hash:
                    by_hash[h]={
                        "encoding":label,
                        "path":p,
                        "sources":{root_name},
                        "sha256":h,
                    }
                else:
                    by_hash[h]["sources"].add(root_name)
                    # If duplicate corpora disagree, drop rather than reconcile silently.
                    if by_hash[h]["encoding"] != label:
                        by_hash[h]["conflict"]=True

    rows=[v for v in by_hash.values() if not v.get("conflict")]
    return rows,source_counts


def utf8_valid(data: bytes) -> float:
    try:
        data.decode("utf-8")
        return 1.0
    except UnicodeDecodeError:
        return 0.0


def feature_vector(path: Path) -> np.ndarray:
    data=path.read_bytes()
    sample=data[:4096]
    a=np.frombuffer(sample,dtype=np.uint8)
    n=max(1,len(sample))
    ext=path.suffix.lower()
    lines=sample.splitlines() or [b""]
    lens=np.array([len(x) for x in lines],dtype=np.float32)
    mean=float(lens.mean()) if len(lens) else 0.0
    counts=Counter(int(x) for x in lens)
    repeated=sum(c for c in counts.values() if c>=2)/max(1,len(lines))

    vals={
        "known_markup_ext":float(ext in MARKUP_EXT),
        "known_record_ext":float(ext in RECORD_EXT),
        "known_code_ext":float(ext in CODE_EXT),
        "known_text_ext":float(ext in TEXT_EXT),
        "log_len":math.log2(len(data)+1)/20.0,
        "ascii_only_4k":float(bool(sample) and all(b<128 for b in sample)),
        "utf8_valid_4k":utf8_valid(sample),
        "high_byte_ratio":float(np.mean(a>=128)) if len(a) else 0.0,
        "nul_ratio":sample.count(b"\x00")/n,
        "lt_ratio":sample.count(b"<")/n,
        "slash_ratio":sample.count(b"/")/n,
        "quote_ratio":(sample.count(b'"')+sample.count(b"'"))/n,
        "comma_ratio":sample.count(b",")/n,
        "newline_ratio":sample.count(b"\n")/n,
        "line_repeat_ratio":repeated,
        "line_len_cv":float(lens.std()/(mean+1.0)) if len(lens) else 0.0,
    }
    return np.array([vals[f] for f in FEATURES],dtype=np.float32)


def entropy(labels: np.ndarray) -> float:
    if len(labels)==0:
        return 0.0
    counts=np.array(list(Counter(labels).values()),dtype=float)
    p=counts/counts.sum()
    return float(-(p*np.log2(p)).sum())


def candidate_thresholds(col: np.ndarray):
    vals=np.unique(col)
    if len(vals)<=12:
        if len(vals)<=1:
            return []
        return [(float(a)+float(b))/2 for a,b in zip(vals[:-1],vals[1:])]
    qs=np.unique(np.quantile(col,[.1,.2,.3,.4,.5,.6,.7,.8,.9]))
    return [float(x) for x in qs]


@dataclass
class Node:
    indices: np.ndarray
    depth: int
    feature: int | None=None
    threshold: float | None=None
    left: "Node|None"=None
    right: "Node|None"=None
    leaf_id: int | None=None
    dist: dict | None=None


def best_split(node: Node, X: np.ndarray, y: np.ndarray, allowed_features: set[int]):
    idx=node.indices
    if node.depth>=MAX_DEPTH or len(idx)<2*MIN_LEAF:
        return None
    parent_h=entropy(y[idx])
    best=None
    for j in allowed_features:
        col=X[idx,j]
        for t in candidate_thresholds(col):
            lm=col<=t
            nl=int(lm.sum()); nr=len(idx)-nl
            if nl<MIN_LEAF or nr<MIN_LEAF:
                continue
            li=idx[lm]; ri=idx[~lm]
            child_h=(nl/len(idx))*entropy(y[li])+(nr/len(idx))*entropy(y[ri])
            gain=parent_h-child_h
            utility=gain/FEATURE_COST[FEATURES[j]]
            cand=(utility,gain,j,t,li,ri)
            if best is None or cand[:2]>best[:2]:
                best=cand
    return best


def train_tree(X,y,allow_context: bool):
    if allow_context:
        allowed=set(range(len(FEATURES)))
    else:
        allowed={i for i,f in enumerate(FEATURES) if not f.startswith("known_")}

    root=Node(np.arange(len(y)),0)
    leaves=[root]

    while len(leaves)<MAX_LEAVES:
        options=[]
        for leaf in leaves:
            bs=best_split(leaf,X,y,allowed)
            if bs:
                options.append((bs[0],bs[1],leaf,bs))
        if not options:
            break
        _,_,leaf,bs=max(options,key=lambda x:(x[0],x[1]))
        utility,gain,j,t,li,ri=bs
        leaf.feature=j; leaf.threshold=t
        leaf.left=Node(li,leaf.depth+1)
        leaf.right=Node(ri,leaf.depth+1)
        leaves=[x for x in leaves if x is not leaf]
        leaves.extend([leaf.left,leaf.right])

    for lid,leaf in enumerate(leaves):
        leaf.leaf_id=lid
        c=Counter(y[leaf.indices])
        total=sum(c.values())
        leaf.dist={k:v/total for k,v in c.most_common()}
    return root,leaves


def route(root: Node, x: np.ndarray):
    node=root
    cost=0.0
    seen=set()
    path=[]
    while node.feature is not None:
        f=FEATURES[node.feature]
        if f not in seen:
            cost+=FEATURE_COST[f]; seen.add(f)
        go_left=x[node.feature]<=node.threshold
        path.append((f,float(node.threshold),"L" if go_left else "R"))
        node=node.left if go_left else node.right
    return node,cost,path


def serialize(node: Node):
    if node.feature is None:
        return {"leaf":node.leaf_id,"n":len(node.indices),"top":list(node.dist.items())[:8]}
    return {
        "feature":FEATURES[node.feature],
        "threshold":node.threshold,
        "cost":FEATURE_COST[FEATURES[node.feature]],
        "n":len(node.indices),
        "left":serialize(node.left),
        "right":serialize(node.right),
    }


def fold_of(name: str)->int:
    return int.from_bytes(hashlib.sha256(name.encode()).digest()[:8],"big")%FOLDS


def weighted_leaf_entropy(leaves, y):
    n=sum(len(leaf.indices) for leaf in leaves)
    return sum((len(leaf.indices)/n)*entropy(y[leaf.indices]) for leaf in leaves)


def evaluate(rows,X,y,allow_context):
    groups=np.array([fold_of(r["path"].name) for r in rows])
    fold_results=[]
    all_top1=all_top3=all_top5=0
    total=0
    costs=[]
    leaf_entropies=[]
    trees=[]

    for fold in range(FOLDS):
        tr=np.where(groups!=fold)[0]
        te=np.where(groups==fold)[0]
        root,leaves=train_tree(X[tr],y[tr],allow_context)

        # remap leaf distributions to global training labels already embedded in tree
        # tree indices refer to X[tr] positions, so rebuild with local arrays and route test.
        Xtr=X[tr]; ytr=y[tr]
        root,leaves=train_tree(Xtr,ytr,allow_context)
        train_entropy_before=entropy(ytr)
        train_entropy_after=weighted_leaf_entropy(leaves,ytr)
        if fold==0:
            trees.append(serialize(root))

        f1=f3=f5=0
        fold_cost=[]
        for gi in te:
            leaf,cost,_=route(root,X[gi])
            ranked=list(leaf.dist)
            target=y[gi]
            f1+=int(target in ranked[:1])
            f3+=int(target in ranked[:3])
            f5+=int(target in ranked[:5])
            fold_cost.append(cost)
            leaf_entropies.append(entropy(np.array(list(leaf.dist.keys()),dtype=object)))
        n=len(te)
        all_top1+=f1; all_top3+=f3; all_top5+=f5; total+=n; costs+=fold_cost
        fold_results.append({
            "fold":fold,"n":n,
            "top1":f1/n,"top3":f3/n,"top5":f5/n,
            "mean_path_cost":float(np.mean(fold_cost)),
            "train_entropy_before_bits":train_entropy_before,
            "train_entropy_after_bits":train_entropy_after,
            "train_entropy_reduction_bits":train_entropy_before-train_entropy_after,
            "train_entropy_reduction_fraction":(
                (train_entropy_before-train_entropy_after)/train_entropy_before
                if train_entropy_before else 0.0
            ),
        })

    return {
        "top1":all_top1/total,
        "top3":all_top3/total,
        "top5":all_top5/total,
        "mean_path_cost":float(np.mean(costs)),
        "mean_entropy_reduction_fraction":float(np.mean(
            [r["train_entropy_reduction_fraction"] for r in fold_results]
        )),
        "folds":fold_results,
        "fold0_tree":trees[0],
    }


def main():
    rows,source_counts=collect()
    X=np.stack([feature_vector(r["path"]) for r in rows])
    y=np.array([r["encoding"] for r in rows])
    result={
        "raw_source_counts":dict(source_counts),
        "deduped_files":len(rows),
        "encodings":len(set(y)),
        "max_depth":MAX_DEPTH,
        "max_leaves":MAX_LEAVES,
        "features":[{"name":f,"cost":FEATURE_COST[f]} for f in FEATURES],
        "bytes_only":evaluate(rows,X,y,False),
        "context_plus_bytes":evaluate(rows,X,y,True),
    }
    print(json.dumps(result,indent=2,sort_keys=True))


if __name__=="__main__":
    main()
