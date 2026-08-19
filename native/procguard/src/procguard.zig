const std = @import("std");
const builtin = @import("builtin");

pub const Handle = u64;

pub const OK: c_int = 0;
pub const ERR_UNSUPPORTED: c_int = -1;
pub const ERR_SYSTEM: c_int = -2;
pub const ERR_INVALID: c_int = -3;

const INVALID: Handle = 0;

const impl = switch (builtin.os.tag) {
    .windows => windows_impl,
    .linux, .macos, .freebsd, .openbsd, .netbsd => posix_impl,
    else => unsupported_impl,
};

export fn procguard_create(out: ?*Handle) c_int {
    const slot = out orelse return ERR_INVALID;
    return impl.create(slot);
}

export fn procguard_prepare_child() c_int {
    return impl.prepareChild();
}

export fn procguard_adopt(handle: ?*Handle, pid: u64) c_int {
    const slot = handle orelse return ERR_INVALID;
    if (pid == 0) return ERR_INVALID;
    return impl.adopt(slot, pid);
}

export fn procguard_terminate(handle: Handle) c_int {
    if (handle == INVALID) return ERR_INVALID;
    return impl.terminate(handle);
}

export fn procguard_destroy(handle: Handle) c_int {
    if (handle == INVALID) return OK;
    return impl.destroy(handle);
}

const posix_impl = struct {
    const SIGKILL: c_int = 9;
    const ESRCH: c_int = 3;
    const PR_SET_PDEATHSIG: c_int = 1;

    extern fn prctl(option: c_int, arg2: c_ulong, arg3: c_ulong, arg4: c_ulong, arg5: c_ulong) c_int;

    fn create(out: *Handle) c_int {
        out.* = INVALID;
        return OK;
    }

    fn prepareChild() c_int {
        if (std.c.setpgid(0, 0) != 0) return ERR_SYSTEM;
        if (builtin.os.tag == .linux) {
            if (prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) != 0) return ERR_SYSTEM;
        }
        return OK;
    }

    fn adopt(handle: *Handle, pid: u64) c_int {
        handle.* = pid;
        return OK;
    }

    fn signalGroup(handle: Handle, sig: std.c.SIG) c_int {
        const pgid: std.c.pid_t = @intCast(handle);
        if (std.c.kill(-pgid, sig) != 0) {
            if (std.c._errno().* == ESRCH) return OK;
            return ERR_SYSTEM;
        }
        return OK;
    }

    fn terminate(handle: Handle) c_int {
        return signalGroup(handle, std.c.SIG.TERM);
    }

    fn destroy(handle: Handle) c_int {
        return signalGroup(handle, std.c.SIG.KILL);
    }
};

const windows_impl = struct {
    const windows = std.os.windows;

    extern "kernel32" fn CreateJobObjectW(?*anyopaque, ?windows.LPCWSTR) callconv(.winapi) ?windows.HANDLE;
    extern "kernel32" fn SetInformationJobObject(windows.HANDLE, c_int, *anyopaque, windows.DWORD) callconv(.winapi) windows.BOOL;
    extern "kernel32" fn AssignProcessToJobObject(windows.HANDLE, windows.HANDLE) callconv(.winapi) windows.BOOL;
    extern "kernel32" fn TerminateJobObject(windows.HANDLE, windows.UINT) callconv(.winapi) windows.BOOL;
    extern "kernel32" fn OpenProcess(windows.DWORD, windows.BOOL, windows.DWORD) callconv(.winapi) ?windows.HANDLE;

    const JobObjectExtendedLimitInformation: c_int = 9;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: windows.DWORD = 0x00002000;
    const PROCESS_SET_QUOTA: windows.DWORD = 0x0100;
    const PROCESS_TERMINATE: windows.DWORD = 0x0001;

    const IO_COUNTERS = extern struct {
        ReadOperationCount: u64,
        WriteOperationCount: u64,
        OtherOperationCount: u64,
        ReadTransferCount: u64,
        WriteTransferCount: u64,
        OtherTransferCount: u64,
    };

    const JOBOBJECT_BASIC_LIMIT_INFORMATION = extern struct {
        PerProcessUserTimeLimit: windows.LARGE_INTEGER,
        PerJobUserTimeLimit: windows.LARGE_INTEGER,
        LimitFlags: windows.DWORD,
        MinimumWorkingSetSize: usize,
        MaximumWorkingSetSize: usize,
        ActiveProcessLimit: windows.DWORD,
        Affinity: usize,
        PriorityClass: windows.DWORD,
        SchedulingClass: windows.DWORD,
    };

    const JOBOBJECT_EXTENDED_LIMIT_INFORMATION = extern struct {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION,
        IoInfo: IO_COUNTERS,
        ProcessMemoryLimit: usize,
        JobMemoryLimit: usize,
        PeakProcessMemoryUsed: usize,
        PeakJobMemoryUsed: usize,
    };

    fn create(out: *Handle) c_int {
        out.* = INVALID;
        const job = CreateJobObjectW(null, null) orelse return ERR_SYSTEM;

        var info = std.mem.zeroes(JOBOBJECT_EXTENDED_LIMIT_INFORMATION);
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if (SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info,
            @sizeOf(JOBOBJECT_EXTENDED_LIMIT_INFORMATION),
        ) == .FALSE) {
            windows.CloseHandle(job);
            return ERR_SYSTEM;
        }

        out.* = @intFromPtr(job);
        return OK;
    }

    fn prepareChild() c_int {
        return OK;
    }

    fn adopt(handle: *Handle, pid: u64) c_int {
        const job: windows.HANDLE = @ptrFromInt(handle.*);
        const access = PROCESS_SET_QUOTA | PROCESS_TERMINATE;
        const process = OpenProcess(access, .FALSE, @intCast(pid)) orelse return ERR_SYSTEM;
        defer windows.CloseHandle(process);
        if (AssignProcessToJobObject(job, process) == .FALSE) return ERR_SYSTEM;
        return OK;
    }

    fn terminate(handle: Handle) c_int {
        const job: windows.HANDLE = @ptrFromInt(handle);
        if (TerminateJobObject(job, 1) == .FALSE) return ERR_SYSTEM;
        return OK;
    }

    fn destroy(handle: Handle) c_int {
        const job: windows.HANDLE = @ptrFromInt(handle);
        windows.CloseHandle(job);
        return OK;
    }
};

const unsupported_impl = struct {
    fn create(out: *Handle) c_int {
        out.* = INVALID;
        return ERR_UNSUPPORTED;
    }
    fn prepareChild() c_int {
        return ERR_UNSUPPORTED;
    }
    fn adopt(handle: *Handle, pid: u64) c_int {
        _ = handle;
        _ = pid;
        return ERR_UNSUPPORTED;
    }
    fn terminate(handle: Handle) c_int {
        _ = handle;
        return ERR_UNSUPPORTED;
    }
    fn destroy(handle: Handle) c_int {
        _ = handle;
        return ERR_UNSUPPORTED;
    }
};

test "create yields a usable handle slot" {
    var handle: Handle = 12345;
    try std.testing.expectEqual(OK, procguard_create(&handle));
}

test "null pointers are rejected rather than dereferenced" {
    try std.testing.expectEqual(ERR_INVALID, procguard_create(null));
    try std.testing.expectEqual(ERR_INVALID, procguard_adopt(null, 1));
}

test "adopting pid zero is rejected" {
    var handle: Handle = INVALID;
    try std.testing.expectEqual(ERR_INVALID, procguard_adopt(&handle, 0));
}

test "terminating an invalid handle is rejected" {
    try std.testing.expectEqual(ERR_INVALID, procguard_terminate(INVALID));
}

test "destroying an invalid handle is a no-op" {
    try std.testing.expectEqual(OK, procguard_destroy(INVALID));
}

test "terminating a group with no members succeeds" {
    if (builtin.os.tag == .windows) return error.SkipZigTest;
    var handle: Handle = INVALID;
    try std.testing.expectEqual(OK, procguard_create(&handle));
    try std.testing.expectEqual(OK, procguard_adopt(&handle, 0x7fffffff));
    try std.testing.expectEqual(OK, procguard_terminate(handle));
}
