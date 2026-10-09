# Linux x86 X3 Final Evidence Decision — 2026-10-09

## Evidence
Same-run paired X3: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37890053390
Foundation PASS: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37869233762
Generic STOP holdout: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37706179018

Six Linux x86 GitHub-hosted runner allocations; three workload families x six pressure regimes = 108 paired observations. 34 oracle-recoverable opportunities and 74 no-opportunity cases. Successful data-quality gate after correcting PSI parser (prior run 37882151015 had zero valid PSI signal pairs and is invalid evidence).

## X3 measured results
PSI some delta / elapsed time, measured over each workload execution:
- CPU PSI opportunity median 0.17280, no-opportunity median 0.07519, AUC 0.6649.
- Memory PSI medians both 0, AUC 0.5000.
- IO PSI medians both 0, AUC 0.4690.
- CPU PSI opportunity range 0.0233–0.3458; no-opportunity range 0.0095–0.2895, substantial overlap.

Descriptive sensitivity analysis (NOT recommended thresholds):
- Proxy F0: non-baseline regime, tp/fp/fn/tn = 34/56/0/18; recall 100%, no-opportunity pruning 24.3%. This is NOT a literal replay of actual F0 resource-transition events.
- CPU PSI >0.10: 20/27/14/47; recall 58.8%, pruning 63.5%.
- CPU PSI >0.20: 13/16/21/58; recall 38.2%, pruning 78.4%.
- CPU PSI positive (>0): 34/74/0/0; no pruning.
- These thresholds were examined on the same data. No holdout generalization may be claimed.

Opportunity counts by regime:
baseline-full 0/18; combined 12/18; cpu-pressure 7/18; memory-heavy 4/18; memory-light 5/18; recovery 6/18.
By workload: w1 8/36, w2 18/36, w3 8/36.

## Limitations preventing economic GO
1. PSI recorded over the same execution used to label oracle opportunity (contemporaneous/post-event association). There is no pre-decision lead-time evidence.
2. Individual CPU PSI read/normalization costs and costs of intervention avoided have NOT been measured. Cannot assert net-positive eligibility economics.
3. GitHub-hosted virtualization and synthetic memory loads do not validate physical server-native events, NUMA or reclaim.
4. Regime/workload confounding and six distinct host allocations limit transfer inference. AUC 0.665 is descriptive, not an independently validated predictor.
5. No production hot-path change or specialized implementation.

## Decisions
- Foundation: PASS / frozen.
- Generic autonomous optimizer: STOP / frozen.
- X1: inventory complete (4 ubuntu-slim hosts lacked CPU PSI).
- X2: synthetic CPU contention observability YES; meaningful memory and IO stall sensitivity NOT established.
- X3: paired descriptive analysis COMPLETE. **Economic GO: NOT ESTABLISHED.**
- **STOP** universal host CPU PSI threshold/estimator policy development. Do not tune CPU-specific constants on X3.
- **NEXT: physical Linux x86 native-event validation** (Intel HFI where available; AMD CPPC / topology path) with low-overhead pre-decision event capture and paired local oracle checks. If native events are absent, use cgroup-specific PSI/event experiments on a controlled physical host, not another generic random-host threshold sweep.
- Single bundled experiment: availability -> cheap pre-event timestamp -> bounded route opportunity measurement -> signal cost -> matched negative controls -> per-host validation -> GO/STOP. Report all outcomes in one artifact; no interim user decision gates.
