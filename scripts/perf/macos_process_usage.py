"""Read Apple proc_pid_rusage counters for native performance captures."""

import ctypes
import os
from functools import lru_cache

RUSAGE_INFO_V4 = 4


class RusageInfoV4(ctypes.Structure):
    # Layout from the Darwin SDK's sys/resource.h.
    _fields_ = [
        ("uuid", ctypes.c_uint8 * 16),
        ("user_time", ctypes.c_uint64),
        ("system_time", ctypes.c_uint64),
        ("wakeups_and_resident_counters", ctypes.c_uint64 * 5),
        ("physical_footprint", ctypes.c_uint64),
        ("lifetime_and_child_counters", ctypes.c_uint64 * 8),
        ("disk_bytes_read", ctypes.c_uint64),
        ("disk_bytes_written", ctypes.c_uint64),
        ("qos_and_logical_writes", ctypes.c_uint64 * 10),
        ("peak_physical_footprint", ctypes.c_uint64),
        ("remaining_counters", ctypes.c_uint64 * 6),
    ]


class MachTimebaseInfo(ctypes.Structure):
    _fields_ = [("numerator", ctypes.c_uint32), ("denominator", ctypes.c_uint32)]


@lru_cache(maxsize=1)
def _libproc():
    lib = ctypes.CDLL(None, use_errno=True)
    lib.proc_pid_rusage.argtypes = (
        ctypes.c_int,
        ctypes.c_int,
        ctypes.POINTER(RusageInfoV4),
    )
    lib.proc_pid_rusage.restype = ctypes.c_int
    lib.proc_listchildpids.argtypes = (ctypes.c_int, ctypes.c_void_p, ctypes.c_int)
    lib.proc_listchildpids.restype = ctypes.c_int
    return lib


@lru_cache(maxsize=1)
def _mach_timebase():
    info = MachTimebaseInfo()
    timebase_info = _libproc().mach_timebase_info
    timebase_info.argtypes = (ctypes.POINTER(MachTimebaseInfo),)
    timebase_info.restype = ctypes.c_int
    status = timebase_info(ctypes.byref(info))
    if status or not info.denominator:
        raise RuntimeError(f"mach_timebase_info failed with status {status}")
    return info.numerator, info.denominator


def process_usage(pid):
    """Return cumulative CPU and I/O counters plus current and peak memory."""
    sample = RusageInfoV4()
    if _libproc().proc_pid_rusage(pid, RUSAGE_INFO_V4, ctypes.byref(sample)) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), pid)
    numerator, denominator = _mach_timebase()
    return {
        "cpu_ns": (sample.user_time + sample.system_time) * numerator // denominator,
        "physical_footprint_bytes": sample.physical_footprint,
        "peak_physical_footprint_bytes": sample.peak_physical_footprint,
        "disk_bytes_read": sample.disk_bytes_read,
        "disk_bytes_written": sample.disk_bytes_written,
    }


def child_pids(pid):
    """List immediate children such as the Desktop backend."""
    lib = _libproc()
    capacity = 16
    while True:
        children = (ctypes.c_int * capacity)()
        count = lib.proc_listchildpids(pid, children, ctypes.sizeof(children))
        if count < 0:
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error), pid)
        if count < capacity:
            return children[:count]
        capacity *= 2
