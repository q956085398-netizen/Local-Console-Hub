# A hosted run must not put a console window on the desktop (D-035).
#
# What this measures: how many console-host windows the desktop can see while a
# supervised run is alive. It starts the crate's own test binary with no console
# of its own -- the shape the packaged Hub has -- because that is the condition
# the defect needs: a console-subsystem child of a process without a console is
# given one by Windows, and on a machine whose default terminal application is
# Windows Terminal (or conhost) that console is shown as a window titled after
# the program being run.
#
# Usage:
#   cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run
#   powershell -NoProfile -File scripts\verify-supervised-console-windows.ps1
#
# To verify graceful delivery on a shared console that has no window:
#   powershell -NoProfile -File scripts\verify-supervised-console-windows.ps1 `
#     -WindowlessRunnerConsole `
#     -TestFilter process::tests::a_graceful_stop_still_reaches_a_run_whose_console_has_no_window
# This gives the runner a windowless console for its run to inherit. It does not
# establish graceful delivery by AttachConsole from a console-less Hub.
#
# To see the defect, build the binary with the console flag removed (make
# `creation_flags_for` return `flags` for both branches in
# src-tauri/src/process/win.rs) and run this script again: the same run then
# reports one visible console window titled `C:\WINDOWS\system32\cmd.exe`.
#
# A run that never allocates a console is invisible to this measurement, so it
# cannot prove anything about the ConPTY path -- a terminal shell is on a
# pseudoconsole and never had a window to lose.
#
# The runner having no console of its own is also what makes this measurement
# noisy: from that state *every* console child the test binary starts is given a
# console, including the throwaway ones tests spawn for their own reasons
# (`taskkill`, a PowerShell helper, a fixture executable). Windows for those say
# nothing about the supervised path, which is why the default filter names one
# supervised test rather than a whole suite.
#
# What it cannot prove: that a real Hub process behaves the same way (the
# condition is reproduced with a console-less test binary, not with the packaged
# app), nor anything about the tray, the taskbar or window decorations.
param(
    # The crate test binary to run with no console. Defaults to the newest one
    # under src-tauri/target/debug/deps.
    [string] $TestBinary,
    # Which test to run. The default holds a supervised run alive long enough for
    # a window to be seen, which the short-lived ones are too fast for.
    [ValidatePattern('^[A-Za-z0-9_:]+$')]
    [string] $TestFilter = 'process::tests::stop_ends_a_live_run_and_leaves_nothing_in_the_tree',
    [switch] $WindowlessRunnerConsole,
    [ValidateRange(10, 1000)]
    [int] $PollMilliseconds = 250,
    [ValidateRange(1, 600)]
    [int] $TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'

if (-not $TestBinary) {
    $binaries = Get-ChildItem -Path (Join-Path $PSScriptRoot '..\src-tauri\target\debug\deps') `
        -Filter 'local_console_hub_lib-*.exe' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending
    if (-not $binaries) {
        throw 'no test binary found -- run: cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run'
    }
    $TestBinary = $binaries[0].FullName
}
$TestBinary = (Resolve-Path -LiteralPath $TestBinary).Path

# The Windows Terminal hosting class and the classic conhost one. Which of the
# two draws the window is a machine setting, so both are counted; the window
# classes are the hosts' own, so desktop chrome owned by the same process is not.
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class ConsoleWindows {
    [StructLayout(LayoutKind.Sequential)]
    public struct STARTUPINFO {
        public int cb;
        public IntPtr lpReserved, lpDesktop, lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }

    [StructLayout(LayoutKind.Sequential)]
    public struct SECURITY_ATTRIBUTES { public int nLength; public IntPtr lpSecurityDescriptor; public bool bInheritHandle; }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta,
        bool inherit, uint flags, IntPtr env, string cwd, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateFileW(string name, uint access, uint share, ref SECURITY_ATTRIBUTES sa,
        uint disposition, uint attributes, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr h, out uint code);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr h);

    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] static extern int GetWindowTextLengthW(IntPtr hWnd);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);

    // DETACHED_PROCESS: the runner has no console at all, which is what makes a
    // console-subsystem child allocate one.
    const uint DETACHED_PROCESS = 0x00000008;
    const uint CREATE_NO_WINDOW = 0x08000000;
    const uint STARTF_USESTDHANDLES = 0x00000100;

    public sealed class Window {
        public string Id;
        public string Description;
    }

    /// Identity is the window handle plus owner pid; a title change is not a
    /// new window. Descriptions are only for the human-readable evidence.
    public static List<Window> Visible() {
        var rows = new List<Window>();
        EnumWindows(delegate(IntPtr hWnd, IntPtr lParam) {
            var cls = new StringBuilder(256);
            GetClassNameW(hWnd, cls, cls.Capacity);
            string klass = cls.ToString();
            if (klass != "ConsoleWindowClass" && klass != "CASCADIA_HOSTING_WINDOW_CLASS") return true;
            if (!IsWindowVisible(hWnd)) return true;
            uint pid;
            GetWindowThreadProcessId(hWnd, out pid);
            string owner = "?";
            try { owner = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName; } catch { }
            int len = GetWindowTextLengthW(hWnd);
            var title = new StringBuilder(len + 1);
            GetWindowTextW(hWnd, title, title.Capacity);
            rows.Add(new Window {
                Id = string.Format("{0:X}|{1}", hWnd.ToInt64(), pid),
                Description = string.Format("{0}|pid={1}|{2}|title=[{3}]", owner, pid, klass, title)
            });
            return true;
        }, IntPtr.Zero);
        return rows;
    }

    /// The pid of the run started by `RunAsync`, so a run that outlives
    /// the measurement can be stopped instead of left spawning windows.
    public static int LastPid;

    /// Start the command with the selected console shape and output in `logPath`, and hand
    /// back a task that completes when it exits.
    public static System.Threading.Tasks.Task<int> RunAsync(string commandLine, string cwd, string logPath, bool hasConsole) {
        return System.Threading.Tasks.Task.Run(() => {
            var sa = new SECURITY_ATTRIBUTES();
            sa.nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES));
            sa.bInheritHandle = true;
            IntPtr log = CreateFileW(logPath, 0x40000000, 0x00000003, ref sa, 2, 0x80, IntPtr.Zero);
            if (log == new IntPtr(-1)) {
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            }
            var si = new STARTUPINFO();
            si.cb = Marshal.SizeOf(typeof(STARTUPINFO));
            si.dwFlags = (int)STARTF_USESTDHANDLES;
            si.hStdInput = IntPtr.Zero;
            si.hStdOutput = log;
            si.hStdError = log;
            PROCESS_INFORMATION pi;
            var cmd = new StringBuilder(commandLine);
            uint flags = hasConsole ? CREATE_NO_WINDOW : DETACHED_PROCESS;
            if (!CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, true, flags,
                    IntPtr.Zero, cwd, ref si, out pi)) {
                int error = Marshal.GetLastWin32Error();
                CloseHandle(log);
                throw new System.ComponentModel.Win32Exception(error);
            }
            LastPid = pi.dwProcessId;
            try {
                if (WaitForSingleObject(pi.hProcess, 0xFFFFFFFF) != 0) {
                    throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
                }
                uint code;
                if (!GetExitCodeProcess(pi.hProcess, out code)) {
                    throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
                }
                return (int)code;
            } finally {
                CloseHandle(pi.hProcess);
                CloseHandle(pi.hThread);
                CloseHandle(log);
            }
        });
    }
}
"@

$logPath = Join-Path $env:TEMP ("lch-console-windows-{0}.log" -f $PID)
$baseline = [ConsoleWindows]::Visible()
Write-Output ("runner: {0}" -f $TestBinary)
Write-Output ("filter: {0}" -f $TestFilter)
Write-Output ("baseline_visible_console_windows={0}" -f $baseline.Count)
foreach ($row in $baseline) { Write-Output ("  baseline {0}" -f $row.Description) }

$commandLine = '"{0}" "{1}" --exact --test-threads=1 --nocapture' -f $TestBinary, $TestFilter
Write-Output ("runner_console={0}" -f $(if ($WindowlessRunnerConsole) { 'windowless' } else { 'none' }))
$task = [ConsoleWindows]::RunAsync($commandLine, (Get-Location).Path, $logPath, $WindowlessRunnerConsole.IsPresent)

$seen = New-Object System.Collections.Generic.HashSet[string]
foreach ($row in $baseline) { [void] $seen.Add($row.Id) }
$appeared = New-Object System.Collections.Generic.HashSet[string]
$timer = [System.Diagnostics.Stopwatch]::StartNew()
while (!$task.IsCompleted -and $timer.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
    Start-Sleep -Milliseconds $PollMilliseconds
    foreach ($row in [ConsoleWindows]::Visible()) {
        if ($seen.Add($row.Id)) {
            [void] $appeared.Add($row.Id)
            Write-Output ("APPEARED after {0}ms: {1}" -f $timer.ElapsedMilliseconds, $row.Description)
        }
    }
}
$timedOut = !$task.IsCompleted
if (!$task.IsCompleted) {
    # A run that outlives the measurement is stopped rather than left behind: it
    # has no console of its own, so every console child it starts from here on
    # would add a window to the desktop after the measurement had ended.
    Write-Output ("runner exceeded {0}s and is being stopped" -f $TimeoutSeconds)
    if ([ConsoleWindows]::LastPid -gt 0) {
        Stop-Process -Id ([ConsoleWindows]::LastPid) -Force -ErrorAction SilentlyContinue
    }
    if (!$task.Wait(5000)) { throw 'runner did not exit after timeout cleanup' }
}
$exit = $task.Result

Write-Output ("runner_exit={0}" -f $exit)
Write-Output ("appeared_visible_console_windows={0}" -f $appeared.Count)
$testOutput = if (Test-Path $logPath) { Get-Content -LiteralPath $logPath -Raw } else { '' }
if ($testOutput -match 'test result: ([^\r\n]+)') {
    Write-Output ("test_result: {0}" -f $Matches[1].Trim())
} else {
    Write-Output 'test_result: <none reported -- see the log>'
}
Write-Output ("log: {0}" -f $logPath)

# A misspelled filter must not turn an empty run into apparent acceptance.
# Fail on a failed/ignored/missing test, timeout, or any newly visible host.
$onePassed = $testOutput -match 'test result: ok\. 1 passed; 0 failed; 0 ignored;'
$gracefulVerified = $true
if ($TestFilter -eq 'process::tests::a_graceful_stop_still_reaches_a_run_whose_console_has_no_window') {
    $gracefulVerified = $testOutput.Contains('stop_report=StopReport { outcome: Exited, exit: ExitStatus { code: Some(0) }, graceful_delivered: true }')
    if ($WindowlessRunnerConsole) {
        $gracefulVerified = $gracefulVerified -and $testOutput.Contains('hub_has_console=true; shares_hub_console=true;')
    }
    Write-Output ("graceful_verified={0}" -f $gracefulVerified)
}
if ($timedOut -or $exit -ne 0 -or !$onePassed -or !$gracefulVerified -or $appeared.Count -ne 0) {
    Write-Output 'measurement_result=FAIL'
    exit 1
}
Write-Output 'measurement_result=PASS'
