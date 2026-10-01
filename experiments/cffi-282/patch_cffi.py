#!/usr/bin/env python3
import argparse
from pathlib import Path

p=argparse.ArgumentParser()
p.add_argument("tree")
p.add_argument("--kind", choices=["base","fast"], required=True)
args=p.parse_args()
path=Path(args.tree)/"src/c/_cffi_backend.c"
s=path.read_text()

helper=r'''
#include <dlfcn.h>

typedef unsigned int (*swiss_cffi_route_fn)(unsigned int, unsigned int);

static unsigned int
swiss_cffi_mode(void)
{
    const char *value = getenv("SWISS_CFFI_MODE");
    if (value == NULL || *value == '\0')
        return 0;
    return (unsigned int)strtoul(value, NULL, 10);
}

static unsigned int
swiss_cffi_route_bridge(unsigned int plan, unsigned int mode)
{
    const char *path = getenv("SWISS_CFFI_GATE_LIB");
    void *handle;
    swiss_cffi_route_fn route;
    unsigned int result;

    if (path == NULL || *path == '\0')
        return 0;

    handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL)
        return 0;

    route = (swiss_cffi_route_fn)dlsym(handle, "swiss_cffi_route");
    if (route == NULL) {
        dlclose(handle);
        return 0;
    }

    result = route(plan, mode);
    dlclose(handle);
    return result;
}

'''

if args.kind=="fast":
    old="""    unsigned int fast_plan;
    unsigned char result_kind;
    Py_ssize_t exchange_offset_arg[1];
"""
    new="""    unsigned int fast_plan;
    unsigned char result_kind;
    unsigned char swiss_route;
    Py_ssize_t exchange_offset_arg[1];
"""
    if old not in s: raise SystemExit("fast struct anchor missing")
    s=s.replace(old,new,1)

    anchor="""static int
fast_plan_supported(unsigned int plan)
"""
    if anchor not in s: raise SystemExit("fast helper anchor missing")
    s=s.replace(anchor,helper+anchor,1)

    old="""    use_fast_plan = (fvarargs == NULL &&
                     fast_plan_supported(cif_descr->fast_plan) &&
                     direct_plan_applicable(cif_descr, args));
"""
    new="""    {
        unsigned char route = cif_descr->swiss_route;

        if (route == 2 || route == 3) {
            int native_supported = fast_plan_supported(cif_descr->fast_plan);
            int swiss_supported = (route == 2);
            if (swiss_supported != native_supported) {
                PyErr_SetString(PyExc_SystemError,
                                "Swiss/CFFI fast-route shadow mismatch");
                goto error;
            }
            use_fast_plan = (fvarargs == NULL &&
                             swiss_supported &&
                             direct_plan_applicable(cif_descr, args));
        }
        else {
            use_fast_plan = (fvarargs == NULL &&
                             route == 1 &&
                             direct_plan_applicable(cif_descr, args));
        }
    }
"""
    if old not in s: raise SystemExit("fast call selector anchor missing")
    s=s.replace(old,new,1)

    old="""    cif_descr = (cif_description_t *)buffer;

    /* use `ffi_prep_cif_var` if necessary and available */
"""
    new="""    cif_descr = (cif_description_t *)buffer;
    cif_descr->swiss_route = swiss_cffi_route_bridge(cif_descr->fast_plan,
                                              swiss_cffi_mode());

    /* use `ffi_prep_cif_var` if necessary and available */
"""
    if old not in s: raise SystemExit("fast prepare anchor missing")
    s=s.replace(old,new,1)
else:
    old="""    Py_ssize_t exchange_size;
    Py_ssize_t exchange_offset_arg[1];
"""
    new="""    Py_ssize_t exchange_size;
    unsigned char swiss_route;
    Py_ssize_t exchange_offset_arg[1];
"""
    if old not in s: raise SystemExit("base struct anchor missing")
    s=s.replace(old,new,1)

    anchor="""static PyObject*
cdata_call(CDataObject *cd, PyObject *args, PyObject *kwds)
"""
    if anchor not in s: raise SystemExit("base call anchor missing")
    s=s.replace(anchor,helper+anchor,1)

    old="""    if (cif_descr != NULL) {
        /* regular case: this function does not take '...' arguments */
"""
    new="""    if (cif_descr != NULL) {
        if (cif_descr->swiss_route != 0) {
            PyErr_SetString(PyExc_SystemError, "unexpected Swiss base route");
            return NULL;
        }
        /* regular case: this function does not take '...' arguments */
"""
    if old not in s: raise SystemExit("base call route anchor missing")
    s=s.replace(old,new,1)

    old="""    cif_descr = (cif_description_t *)buffer;

    /* use `ffi_prep_cif_var` if necessary and available */
"""
    new="""    cif_descr = (cif_description_t *)buffer;
    cif_descr->swiss_route = swiss_cffi_route_bridge(0, swiss_cffi_mode());

    /* use `ffi_prep_cif_var` if necessary and available */
"""
    if old not in s: raise SystemExit("base prepare anchor missing")
    s=s.replace(old,new,1)

path.write_text(s)
print(f"patched {path} kind={args.kind}")
