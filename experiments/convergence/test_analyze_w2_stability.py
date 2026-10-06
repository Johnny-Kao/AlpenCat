#!/usr/bin/env python3
import importlib.util, pathlib, unittest
HERE=pathlib.Path(__file__).resolve().parent
SPEC=importlib.util.spec_from_file_location("stab",HERE/"analyze_w2_stability.py")
M=importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(M)

def row(n,s,c):
    return {"work_items":n,"serial_samples_ns":[s,s,s],"cpu_samples_ns":[c,c,c]}

class StabilityTests(unittest.TestCase):
    def test_interval_captures_serial_cpu_serial(self):
        points=[row(1,10,20),row(2,20,10),row(3,10,20)]
        p=M.profile(points)
        self.assertEqual(p["sequence"],["Serial","Cpu","Serial"])
        self.assertGreater(p["scalar_gap_pct"],0)
        self.assertEqual(p["interval_gap_pct"],0)

if __name__=="__main__":
    unittest.main()
