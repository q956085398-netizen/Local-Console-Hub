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
# To see the defect, build the binary with the console flag removed (make
# `creation_flags_for` return `flags` for both branches in
# src-tauri/src/process/win.rs) and run this script again: the same run then
# reports one visible console window titled `C:\WINDOWS\system32\cmd.exe`.
#
# A run that never allocates a console is invisible to this measurement, so it
# cannot prove anything about the ConPTY path -- a terminal shell is on a
# pseudoconsole and never had a window to lose.
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
    [string] $TestFilter = 'process::tests::stop_ends_a_live_run_and_leaves_nothing_in_the_tree',
    [int] $PollMilliseconds = 250,
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
    const uint STARTF_USESTDHANDLES = 0x00000100;

    /// Every console-host window the desktop can see, as "owner|pid|class|title".
    public static List<string> Visible() {
        var rows = new List<string>();
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
            rows.Add(string.Format("{0}|pid={1}|{2}|title=[{3}]", owner, pid, klass, title));
            return true;
        }, IntPtr.Zero);
        return rows;
    }

    /// Start the command with no console and its output in `logPath`, and hand
    /// back a task that completes when it exits.
    public static System.Threading.Tasks.Task<int> RunDetachedAsync(string commandLine, string cwd, string logPath) {
        return System.Threading.Tasks.Task.Run(() => {
            var sa = new SECURITY_ATTRIBUTES();
            sa.nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES));
            sa.bInheritHandle = true;
            IntPtr log = CreateFileW(logPath, 0x40000000, 0x00000003, ref sa, 2, 0x80, IntPtr.Zero);
            var si = new STARTUPINFO();
            si.cb = Marshal.SizeOf(typeof(STARTUPINFO));
            si.dwFlags = (int)STARTF_USESTDHANDLES;
            si.hStdInput = IntPtr.Zero;
            si.hStdOutput = log;
            si.hStdError = log;
            PROCESS_INFORMATION pi;
            var cmd = new StringBuilder(commandLine);
            if (!CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, true, DETACHED_PROCESS,
                    IntPtr.Zero, cwd, ref si, out pi)) {
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            }
            WaitForSingleObject(pi.hProcess, 0xFFFFFFFF);
            uint code;
            GetExitCodeProcess(pi.hProcess, out code);
            CloseHandle(pi.hProcess);
            CloseHandle(pi.hThread);
            CloseHandle(log);
            return (int)code;
        });
    }
}
"@

$logPath = Join-Path $env:TEMP ("lch-console-windows-{0}.log" -f $PID)
$baseline = [ConsoleWindows]::Visible()
Write-Output ("runner: {0}" -f $TestBinary)
Write-Output ("filter: {0}" -f $TestFilter)
Write-Output ("baseline_visible_console_windows={0}" -f $baseline.Count)
foreach ($row in $baseline) { Write-Output ("  baseline {0}" -f $row) }

$commandLine = '"{0}" "{1}"' -f $TestBinary, $TestFilter
$task = [ConsoleWindows]::RunDetachedAsync($commandLine, (Get-Location).Path, $logPath)

$seen = New-Object System.Collections.Generic.HashSet[string]
$polls = 0
while (!$task.IsCompleted -and ($polls * $PollMilliseconds) -lt ($TimeoutSeconds * 1000)) {
    Start-Sleep -Milliseconds $PollMilliseconds
    $polls++
    foreach ($row in [ConsoleWindows]::Visible()) {
        if ($seen.Add($row)) { Write-Output ("APPEARED after {0}ms: {1}" -f ($polls * $PollMilliseconds), $row) }
    }
}
$exit = $task.Result

Write-Output ("runner_exit={0}" -f $exit)
Write-Output ("appeared_visible_console_windows={0}" -f $seen.Count)
$testOutput = if (Test-Path $logPath) { Get-Content -LiteralPath $logPath -Raw } else { '' }
if ($testOutput -match 'test result: ([^\r\n]+)') {
    Write-Output ("test_result: {0}" -f $Matches[1].Trim())
} else {
    Write-Output 'test_result: <none reported -- see the log>'
}
Write-Output ("log: {0}" -f $logPath)
