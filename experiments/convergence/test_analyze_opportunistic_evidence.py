#!/usr/bin/env python3
import importlib.util, pathlib, unittest
HERE=pathlib.Path(__file__).resolve().parent
SPEC=importlib.util.spec_from_file_location("opp",HERE/"analyze_opportunistic_evidence.py")
M=importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(M)

class OpportunisticEvidenceTests(unittest.TestCase):
    def point(self):
        return {"weight":0.25,"serial_samples_ns":[10,10,10],"cpu_samples_ns":[20,20,20]}
    def test_short_horizon_may_not_collect_sample(self):
        r=M.simulate_opportunistic(self.point(),100,0.01)
        self.assertFalse(r["evidence_acquired"])
    def test_long_horizon_collects_sample(self):
        r=M.simulate_opportunistic(self.point(),1000,0.01)
        self.assertTrue(r["evidence_acquired"])
    def test_sampling_cost_scales_with_rate(self):
        a=M.simulate_opportunistic(self.point(),1000,0.01)["sampling_cost_ns"]
        b=M.simulate_opportunistic(self.point(),1000,0.10)["sampling_cost_ns"]
        self.assertGreater(b,a)

if __name__=="__main__":
    unittest.main()
