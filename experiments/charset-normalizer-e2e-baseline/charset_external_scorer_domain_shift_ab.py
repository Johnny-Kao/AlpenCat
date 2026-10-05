from __future__ import annotations

import codecs
import gzip
import hashlib
import io
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import requests
from warcio.archiveiterator import ArchiveIterator

sys.path.append(str(Path(__file__).parent))

from charset_cost_aware_routing_tree import canonical_encoding, collect
from charset_training_tournament import scorer_features, fit_linear, rank_model

CRAWL = "CC-MAIN-2026-39"
WARC_PATHS_URL = f"https://data.commoncrawl.org/crawl-data/{CRAWL}/warc.paths.gz"
DATA_ROOT = "https://data.commoncrawl.org/"
N_WARC_FILES = 32
RANGE_BYTES = 2 * 1024 * 1024
MAX_ACCEPTED = 320
MAX_ROUTE = {"U": 80, "N": 40, "R": 220}
TARGET_RESIDUAL = 180
S3 = 0.02
TOPK = (1, 3, 5)
UA = "charset-detector-research/0.1 (GitHub Actions scorer domain-shift A/B)"

META_CHARSET_RE = re.compile(
    br'<meta[^>]+charset\s*=\s*["\']?\s*([a-zA-Z0-9._:+-]+)',
    re.I,
)
META_HTTP_EQUIV_RE = re.compile(
    br'<meta[^>]+http-equiv\s*=\s*["\']?content-type["\']?[^>]+content\s*=\s*["\'][^"\']*charset\s*=\s*([a-zA-Z0-9._:+-]+)',
    re.I,
)
XML_DECL_RE = re.compile(
    br'<\?xml[^>]+encoding\s*=\s*["\']\s*([a-zA-Z0-9._:+-]+)',
    re.I,
)
HEADER_CHARSET_RE = re.compile(r"charset\s*=\s*[\"']?\s*([a-zA-Z0-9._:+-]+)", re.I)


def normalize_label(raw):
    if raw is None:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("ascii", "ignore")
    s = raw.strip().strip("\"'").lower().replace("_", "-")
    aliases = {
        "utf8": "utf-8",
        "windows-1250": "cp1250",
        "windows-1251": "cp1251",
        "windows-1252": "cp1252",
        "windows-1255": "cp1255",
        "windows-1256": "cp1256",
        "windows-31j": "shift-jis",
        "shift_jis": "shift-jis",
        "sjis": "shift-jis",
        "x-sjis": "shift-jis",
        "gbk": "gb18030",
        "gb2312": "gb18030",
        "x-gbk": "gb18030",
        "ks-c-5601-1987": "euc-kr",
    }
    s = aliases.get(s, s)
    try:
        py = codecs.lookup(s).name.lower().replace("_", "-")
    except LookupError:
        py = s
    pyaliases = {
        "iso8859-1": "iso-8859-1",
        "iso8859-2": "iso-8859-2",
        "iso8859-5": "iso-8859-5",
        "iso8859-6": "iso-8859-6",
        "iso8859-7": "iso-8859-7",
        "iso8859-8": "iso-8859-8",
        "iso8859-9": "iso-8859-9",
        "iso8859-15": "iso-8859-15",
        "cp932": "shift-jis",
        "cp950": "big5",
    }
    return pyaliases.get(py, py)


def bom_label(data):
    if data.startswith(b"\xef\xbb\xbf"):
        return "utf-8-sig"
    if data.startswith(b"\xff\xfe\x00\x00"):
        return "utf-32-le"
    if data.startswith(b"\x00\x00\xfe\xff"):
        return "utf-32-be"
    if data.startswith(b"\xff\xfe"):
        return "utf-16-le"
    if data.startswith(b"\xfe\xff"):
        return "utf-16-be"
    return None


def declared_body_label(data):
    head = data[:8192]
    found = []
    for rx in (META_CHARSET_RE, META_HTTP_EQUIV_RE, XML_DECL_RE):
        m = rx.search(head)
        if m:
            found.append(normalize_label(m.group(1)))
    found = [x for x in found if x]
    if not found:
        return None
    return found[0] if len(set(found)) == 1 else None


def header_label(http_headers):
    if http_headers is None:
        return None
    ct = http_headers.get_header("Content-Type") or ""
    m = HEADER_CHARSET_RE.search(ct)
    return normalize_label(m.group(1)) if m else None


def strict_decodable(data, label):
    try:
        data.decode(label, "strict")
        return True
    except Exception:
        return False


def choose_ground_truth(data, hdr, body):
    bom = bom_label(data)
    evidence = [x for x in (bom, hdr, body) if x]
    if not evidence:
        return None, None
    label, n = Counter(evidence).most_common(1)[0]
    if n >= 2 and strict_decodable(data, label):
        return label, "A"
    if bom and strict_decodable(data, bom):
        return bom, "A"
    if hdr and strict_decodable(data, hdr) and (body is None or body == hdr):
        return hdr, "B"
    return None, None


def utf8_valid(data):
    try:
        data[:4096].decode("utf-8", "strict")
        return True
    except UnicodeDecodeError:
        return False


def route_bucket(data):
    sample = data[:4096]
    if utf8_valid(sample):
        return "U"
    if b"\x00" in sample:
        return "N"
    a = np.frombuffer(sample, dtype=np.uint8)
    hbr = float(np.mean(a >= 128)) if len(a) else 0.0
    return "RL" if hbr <= S3 else "RH"


def fetch_paths():
    r = requests.get(WARC_PATHS_URL, headers={"User-Agent": UA}, timeout=60)
    r.raise_for_status()
    return [x.strip() for x in gzip.decompress(r.content).decode().splitlines() if x.strip()]


def deterministic_paths(paths):
    scored = sorted((hashlib.sha256((CRAWL + p).encode()).digest(), p) for p in paths)
    return [p for _, p in scored[:N_WARC_FILES]]


def collect_external():
    paths = deterministic_paths(fetch_paths())
    accepted = []
    route_accept = Counter()
    host_count = Counter()
    stats = Counter()

    for path in paths:
        if len(accepted) >= MAX_ACCEPTED or route_accept["R"] >= TARGET_RESIDUAL:
            break
        r = None
        for attempt in range(3):
            try:
                r = requests.get(
                    DATA_ROOT + path,
                    headers={"User-Agent": UA, "Range": f"bytes=0-{RANGE_BYTES-1}"},
                    timeout=90,
                )
                _ = r.content
                break
            except requests.RequestException:
                stats["range_request_retry"] += 1
                r = None
        if r is None:
            stats["range_request_failed"] += 1
            continue
        stats[f"http_{r.status_code}"] += 1
        if r.status_code not in (200, 206):
            continue
        try:
            it = ArchiveIterator(io.BytesIO(r.content), arc2warc=True)
            for record in it:
                if record.rec_type != "response":
                    continue
                stats["responses_seen"] += 1
                try:
                    data = record.content_stream().read()
                except Exception:
                    stats["body_read_error"] += 1
                    continue
                if not data or len(data) < 32 or len(data) > 2_000_000:
                    continue
                ct = record.http_headers.get_header("Content-Type") if record.http_headers else ""
                ctl = (ct or "").lower()
                if not any(x in ctl for x in ("text/", "html", "xml", "json", "javascript", "css")):
                    continue

                hdr = header_label(record.http_headers)
                body = declared_body_label(data)
                label, tier = choose_ground_truth(data, hdr, body)
                if not label:
                    stats["no_confident_label"] += 1
                    continue

                uri = record.rec_headers.get_header("WARC-Target-URI") or ""
                host = uri.split("/")[2].lower() if "://" in uri and len(uri.split("/")) > 2 else ""
                if host and host_count[host] >= 3:
                    continue

                b = route_bucket(data)
                family = "R" if b in ("RL", "RH") else b
                if route_accept[family] >= MAX_ROUTE[family]:
                    continue

                if host:
                    host_count[host] += 1
                route_accept[family] += 1
                accepted.append({
                    "data": data,
                    "label": normalize_label(canonical_encoding(label)) or label,
                    "tier": tier,
                    "host": host,
                    "route": b,
                    "warc_path": path,
                })
                if len(accepted) >= MAX_ACCEPTED or route_accept["R"] >= TARGET_RESIDUAL:
                    break
        except Exception:
            stats["range_parse_tail_error"] += 1
    return accepted, paths, stats


def load_legacy_training():
    rows, _ = collect()
    labels = np.array([normalize_label(canonical_encoding(r["encoding"])) or r["encoding"] for r in rows], dtype=object)
    X = np.stack([scorer_features(r["path"].read_bytes()) for r in rows])
    buckets = np.array([route_bucket(r["path"].read_bytes()) for r in rows], dtype=object)
    return X, labels, buckets


def eval_fold(train_ext, test_ext, legacy_X, legacy_y, legacy_b):
    results = {}
    for mode in ("legacy_only", "legacy_plus_external"):
        hits = {k: 0 for k in TOPK}
        n = 0
        route_hits = defaultdict(lambda: {k: 0 for k in TOPK})
        route_n = Counter()
        label_hits = defaultdict(lambda: {k: 0 for k in TOPK})
        label_n = Counter()

        for route in ("U", "N", "RL", "RH"):
            li = np.where(legacy_b == route)[0]
            Xtr = legacy_X[li]
            ytr = legacy_y[li]

            if mode == "legacy_plus_external":
                ext_rows = [s for s in train_ext if s["route"] == route]
                if ext_rows:
                    Xext = np.stack([scorer_features(s["data"]) for s in ext_rows])
                    yext = np.array([s["label"] for s in ext_rows], dtype=object)
                    known = np.isin(yext, np.unique(legacy_y))
                    if known.any():
                        Xtr = np.concatenate([Xtr, Xext[known]], axis=0)
                        ytr = np.concatenate([ytr, yext[known]], axis=0)

            if len(Xtr) < 20 or len(np.unique(ytr)) < 2:
                continue

            model = fit_linear(Xtr, ytr, C=0.5)
            for s in [x for x in test_ext if x["route"] == route]:
                if s["label"] not in model[1].classes_:
                    continue
                rank = rank_model(model, scorer_features(s["data"]))
                route_n[route] += 1
                label_n[s["label"]] += 1
                n += 1
                for k in TOPK:
                    hit = int(s["label"] in rank[:k])
                    hits[k] += hit
                    route_hits[route][k] += hit
                    label_hits[s["label"]][k] += hit

        results[mode] = {
            "n": n,
            "top1": hits[1] / max(1, n),
            "top3": hits[3] / max(1, n),
            "top5": hits[5] / max(1, n),
            "score": (4*hits[1] + 2*hits[3] + hits[5]) / max(1, n),
            "route_metrics": {
                r: {
                    "n": route_n[r],
                    **{f"top{k}": route_hits[r][k] / max(1, route_n[r]) for k in TOPK},
                }
                for r in sorted(route_n)
            },
            "label_metrics": {
                lab: {
                    "n": label_n[lab],
                    **{f"top{k}": label_hits[lab][k] / max(1, label_n[lab]) for k in TOPK},
                }
                for lab in sorted(label_n)
            },
        }
    return results


def main():
    external, paths, stats = collect_external()
    legacy_X, legacy_y, legacy_b = load_legacy_training()

    # Domain-held-out by WARC path: train on records from half the sampled WARCs,
    # test on the other half, then swap. This prevents same-WARC memorization.
    used_paths = sorted(set(s["warc_path"] for s in external))
    fold_assign = {p: int.from_bytes(hashlib.sha256(p.encode()).digest()[:8], "big") % 2 for p in used_paths}

    fold_results = []
    agg = {
        "legacy_only": {k: 0.0 for k in ("top1","top3","top5","score")},
        "legacy_plus_external": {k: 0.0 for k in ("top1","top3","top5","score")},
    }
    weights = {"legacy_only": 0, "legacy_plus_external": 0}

    for fold in (0, 1):
        train_ext = [s for s in external if fold_assign[s["warc_path"]] != fold]
        test_ext = [s for s in external if fold_assign[s["warc_path"]] == fold]
        res = eval_fold(train_ext, test_ext, legacy_X, legacy_y, legacy_b)
        fold_results.append({
            "fold": fold,
            "train_external_n": len(train_ext),
            "test_external_n": len(test_ext),
            "results": res,
        })
        for mode in agg:
            n = res[mode]["n"]
            weights[mode] += n
            for k in agg[mode]:
                agg[mode][k] += res[mode][k] * n

    pooled = {}
    for mode in agg:
        n = max(1, weights[mode])
        pooled[mode] = {k: agg[mode][k] / n for k in agg[mode]}
        pooled[mode]["n"] = weights[mode]

    delta = {
        k: pooled["legacy_plus_external"][k] - pooled["legacy_only"][k]
        for k in ("top1","top3","top5","score")
    }

    print(json.dumps({
        "phase": "external_scorer_domain_shift_ab",
        "crawl": CRAWL,
        "selector": {"S1":"utf8 validity","S2":"NUL presence","S3":S3},
        "external_n": len(external),
        "external_routes": dict(Counter(s["route"] for s in external)),
        "external_labels": dict(Counter(s["label"] for s in external).most_common()),
        "external_tiers": dict(Counter(s["tier"] for s in external)),
        "warc_paths_used": used_paths,
        "collection_stats": dict(stats),
        "fold_results": fold_results,
        "pooled": pooled,
        "delta_retrained_vs_legacy": delta,
        "interpretation_rule": {
            "distribution_mismatch_supported": "Meaningful held-out gain from legacy_plus_external with unchanged 518-dim representation.",
            "representation_problem_supported": "Little/no held-out gain after adding real-network training bytes.",
        },
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
