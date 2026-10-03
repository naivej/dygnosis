using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

// The launcher owns this job before the suspended host can create children.
// Root-process exit does not release descendants; only verified cleanup does.
public sealed class DygnosisOwnedHost : IDisposable
{
    private IntPtr job, process, thread, input, output, error;
    public uint pid { get; private set; }
    public bool timed_out { get; private set; }
    public uint? exit_code { get; private set; }
    public bool cleanup_verified { get; private set; }
    public string cleanup_error { get; private set; }

    public DygnosisOwnedHost(string executable, string arguments, string stdout, string stderr)
    {
        try
        {
            job = CreateJobObject(IntPtr.Zero, null);
            Check(job != IntPtr.Zero, "create owned host job");
            var limits = new ExtendedLimits();
            limits.BasicLimitInformation.LimitFlags = 0x2000; // KILL_ON_JOB_CLOSE
            Check(SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf(typeof(ExtendedLimits))), "set owned host job limits");
            input = Open("NUL", 0x80000000, 3);
            output = Open(stdout, 0x40000000, 2);
            error = Open(stderr, 0x40000000, 2);
            var startup = new StartupInfo();
            startup.cb = Marshal.SizeOf(typeof(StartupInfo));
            startup.dwFlags = 0x101; // USESTDHANDLES | USESHOWWINDOW (hidden)
            startup.hStdInput = input; startup.hStdOutput = output; startup.hStdError = error;
            ProcessInformation info;
            Check(CreateProcess(executable, new StringBuilder("\"" + executable + "\" " + arguments),
                IntPtr.Zero, IntPtr.Zero, true, 0x08000004, IntPtr.Zero, null, ref startup, out info), "create suspended host");
            process = info.hProcess; thread = info.hThread; pid = info.dwProcessId;
            Check(AssignProcessToJobObject(job, process), "assign suspended host to owned job");
        }
        catch
        {
            // Assignment can fail on a runner with incompatible outer job rules.
            // The suspended host has not run and cannot have created descendants.
            if (process != IntPtr.Zero) TerminateProcess(process, 1);
            Dispose();
            throw;
        }
    }

    public void Run(uint timeoutMs)
    {
        try
        {
            Check(ResumeThread(thread) != 0xffffffff, "start owned host");
            Close(ref thread);
            uint wait = WaitForSingleObject(process, timeoutMs);
            if (wait == 258) timed_out = true;
            else
            {
                Check(wait == 0, "wait for owned host");
                uint code;
                Check(GetExitCodeProcess(process, out code), "read owned host exit code");
                exit_code = code;
            }
        }
        finally
        {
            Stop();
        }
    }

    private void Stop()
    {
        if (job == IntPtr.Zero || cleanup_verified) return;
        try
        {
            Check(TerminateJobObject(job, 1), "terminate owned host job");
            var deadline = Stopwatch.StartNew();
            while (true)
            {
                var accounting = new BasicAccounting();
                Check(QueryInformationJobObject(job, 1, ref accounting, (uint)Marshal.SizeOf(typeof(BasicAccounting)), IntPtr.Zero), "read owned host job state");
                if (accounting.ActiveProcesses == 0) { cleanup_verified = true; return; }
                if (deadline.ElapsedMilliseconds >= 10000) throw new TimeoutException("Owned host job did not become empty within 10 seconds.");
                Thread.Sleep(20);
            }
        }
        catch (Exception failure)
        {
            cleanup_error = failure.Message;
            throw;
        }
    }

    public void Dispose()
    {
        // Closing the job is also a kernel-enforced stop if the launcher fails.
        Close(ref job); Close(ref thread); Close(ref process);
        Close(ref input); Close(ref output); Close(ref error);
    }
    private static void Close(ref IntPtr handle)
    {
        if (handle != IntPtr.Zero && handle != new IntPtr(-1)) CloseHandle(handle);
        handle = IntPtr.Zero;
    }
    private static void Check(bool condition, string operation)
    {
        if (!condition) throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not " + operation + ".");
    }
    private static IntPtr Open(string path, uint access, uint disposition)
    {
        var security = new SecurityAttributes();
        security.nLength = Marshal.SizeOf(typeof(SecurityAttributes)); security.bInheritHandle = true;
        IntPtr handle = CreateFile(path, access, 3, ref security, disposition, 0x80, IntPtr.Zero);
        Check(handle != new IntPtr(-1), "open host stream");
        return handle;
    }

    [StructLayout(LayoutKind.Sequential)] private struct SecurityAttributes { public int nLength; public IntPtr lpSecurityDescriptor; [MarshalAs(UnmanagedType.Bool)] public bool bInheritHandle; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] private struct StartupInfo
    {
        public int cb; public string lpReserved, lpDesktop, lpTitle;
        public uint dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }
    [StructLayout(LayoutKind.Sequential)] private struct ProcessInformation { public IntPtr hProcess, hThread; public uint dwProcessId, dwThreadId; }
    [StructLayout(LayoutKind.Sequential)] private struct BasicLimits
    {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit; public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize; public uint ActiveProcessLimit;
        public UIntPtr Affinity; public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)] private struct IoCounters { public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount, ReadTransferCount, WriteTransferCount, OtherTransferCount; }
    [StructLayout(LayoutKind.Sequential)] private struct ExtendedLimits { public BasicLimits BasicLimitInformation; public IoCounters IoInfo; public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed; }
    [StructLayout(LayoutKind.Sequential)] private struct BasicAccounting
    {
        public long TotalUserTime, TotalKernelTime, ThisPeriodTotalUserTime, ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses;
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool SetInformationJobObject(IntPtr job, int info, ref ExtendedLimits limits, uint size);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateJobObject(IntPtr job, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool QueryInformationJobObject(IntPtr job, int info, ref BasicAccounting accounting, uint size, IntPtr length);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr CreateFile(string path, uint access, uint sharing, ref SecurityAttributes security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CreateProcess(string file, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string directory, ref StartupInfo startup, out ProcessInformation process);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool CloseHandle(IntPtr handle);
}
