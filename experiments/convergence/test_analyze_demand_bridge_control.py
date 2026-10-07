#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "control", HERE / "analyze_demand_bridge_control.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class DemandBridgeControlTests(unittest.TestCase):
    def test_module_loads(self):
        self.assertTrue(callable(MODULE.main))


if __name__ == "__main__":
    unittest.main()
