import argparse
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("tree", type=Path)
parser.add_argument("--kind", choices=("base", "fast"), required=True)
args = parser.parse_args()

p = args.tree / "src/c/_cffi_backend.c"
text = p.read_text()

anchor = "#include <stdint.h>\n"
if anchor not in text:
    raise SystemExit("stdint anchor not found")

decls = """
extern int runtime_cffi_route(int fast_candidate);
extern unsigned long long runtime_cffi_route_calls(void);
extern unsigned long long runtime_cffi_fast_routes(void);
extern unsigned long long runtime_cffi_legacy_routes(void);
extern int runtime_cffi_shadow_active(void);
extern void runtime_cffi_shadow_record(int mismatch);
extern unsigned long long runtime_cffi_shadow_checks(void);
extern unsigned long long runtime_cffi_shadow_mismatches(void);
extern unsigned long long runtime_cffi_backup_fallbacks(void);
"""
text = text.replace(anchor, anchor + decls, 1)

if args.kind == "fast":
    old = """    use_fast_plan = (fvarargs == NULL &&
                     fast_plan_supported(cif_descr->fast_plan) &&
                     direct_plan_applicable(cif_descr, args));
"""
    new = """    {
        int host_fast_candidate = (fvarargs == NULL &&
                                   fast_plan_supported(cif_descr->fast_plan) &&
                                   direct_plan_applicable(cif_descr, args));
        use_fast_plan = runtime_cffi_route(host_fast_candidate);
    }
"""
    if old not in text:
        raise SystemExit("fast selector anchor not found")
    text = text.replace(old, new, 1)

    helper_anchor = """static PyObject*
cdata_call_impl(CDataObject *cd, PyObject *const *args,
"""
    helper = """static int
swiss_shadow_validate_fast_prepare(cif_description_t *cif_descr,
                                   char *fast_buffer,
                                   PyObject *const *args)
{
    char *legacy_buffer;
    void **legacy_array;
    Py_ssize_t i;
    int mismatch = 0;

    legacy_buffer = PyObject_Malloc(cif_descr->exchange_size);
    if (legacy_buffer == NULL) {
        PyErr_NoMemory();
        return -1;
    }
    legacy_array = (void **)legacy_buffer;

    for (i = 0; i < cif_descr->nargs; i++) {
        CTypeDescrObject *argtype = cif_descr->argtypes[i];
        char *legacy_data =
            legacy_buffer + cif_descr->exchange_offset_arg[1 + i];
        char *fast_data =
            fast_buffer + cif_descr->exchange_offset_arg[1 + i];

        legacy_array[i] = legacy_data;
        if (convert_from_object(legacy_data, argtype, args[i]) < 0) {
            PyObject_Free(legacy_buffer);
            return -1;
        }

        if (argtype->ct_size <= 0 ||
                memcmp(legacy_data, fast_data, (size_t)argtype->ct_size) != 0)
            mismatch = 1;
    }

    runtime_cffi_shadow_record(mismatch);
    PyObject_Free(legacy_buffer);
    return 0;
}

static PyObject*
cdata_call_impl(CDataObject *cd, PyObject *const *args,
"""
    if helper_anchor not in text:
        raise SystemExit("cdata_call_impl anchor not found")
    text = text.replace(helper_anchor, helper, 1)

    prepare_old = """        if (direct_plan_prepared < 0)
            goto error;
        if (direct_plan_prepared > 0)
            goto arguments_ready;
"""
    prepare_new = """        if (direct_plan_prepared < 0)
            goto error;
        if (direct_plan_prepared > 0) {
            if (runtime_cffi_shadow_active() &&
                    swiss_shadow_validate_fast_prepare(cif_descr, buffer, args) < 0)
                goto error;
            goto arguments_ready;
        }
"""
    if prepare_old not in text:
        raise SystemExit("fast prepare anchor not found")
    text = text.replace(prepare_old, prepare_new, 1)
else:
    # The upstream base has only the mature legacy engine.  The Swiss bridge
    # still owns the junction, but advertises no fast candidate.  This isolates
    # the per-call control-plane tax without importing PR #282's vectorcall,
    # stack-buffer, or direct-conversion improvements.
    old = """    buffer = PyObject_Malloc(cif_descr->exchange_size);
"""
    new = """    (void)runtime_cffi_route(0);

    buffer = PyObject_Malloc(cif_descr->exchange_size);
"""
    if old not in text:
        raise SystemExit("legacy buffer anchor not found")
    text = text.replace(old, new, 1)

p.write_text(text)
print("patched", p, "kind=", args.kind)
