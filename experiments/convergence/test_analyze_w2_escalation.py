#!/usr/bin/env python3
import importlib.util,pathlib,tempfile,json,unittest
HERE=pathlib.Path(__file__).resolve().parent
S=importlib.util.spec_from_file_location("e",HERE/"analyze_w2_escalation.py");M=importlib.util.module_from_spec(S);S.loader.exec_module(M)

def p(reg,n,s,c):
 return {"record_type":"point","regime":reg,"work_items":n,"serial_samples_ns":[s]*3,"cpu_samples_ns":[c]*3}
def rv(reg,cost):
 return {"record_type":"revalidation","regime":reg,"revalidation_elapsed_ns":cost}

class T(unittest.TestCase):
 def test_serial_sentinel_can_recover_stale_cpu_route(self):
  with tempfile.TemporaryDirectory() as d:
   root=pathlib.Path(d); ev=root/"ev"; se=root/"se"; ev.mkdir(); se.mkdir()
   (ev/"baseline-full.jsonl").write_text("\n".join(json.dumps(x) for x in [p("baseline-full",10,10,20),p("baseline-full",20,30,10)])+"\n")
   for reg in ("half","contention"):
    (ev/f"{reg}.jsonl").write_text("\n".join(json.dumps(x) for x in [p(reg,10,10,20),p(reg,20,10,30)])+"\n")
    (se/f"{reg}-p3.jsonl").write_text(json.dumps(rv(reg,5))+"\n")
   static,rows=M.analyze(ev,se,0)
   self.assertEqual(static,10)
   self.assertTrue(all(r["candidate_boundary"]==20 for r in rows))
   self.assertTrue(all(r["candidate_capture_fraction"]==1.0 for r in rows))
if __name__=="__main__": unittest.main()
