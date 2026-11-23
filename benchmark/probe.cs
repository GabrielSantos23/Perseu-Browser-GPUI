// Windows numbers for the benchmarks (run.mjs, through probe.ps1): when a process shows its first
// window, and, for a process and all its children, CPU time, context switches (Windows' closest
// thing to macOS's idle wake-ups) and private working set (Task Manager's "Memory"). The perseu
// has no remote control of its own, so this probe also types for it: SendInput of key chords, plus
// the clipboard, are the only way in without touching its code.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;

public static class Probe {
  delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lparam);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll")] static extern uint SendInput(uint count, INPUT[] inputs, int size);
  [DllImport("winmm.dll")] static extern uint timeBeginPeriod(uint ms);
  [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int cls, IntPtr buffer, int length, out int needed);
  [StructLayout(LayoutKind.Sequential)] struct RECT { public int Left, Top, Right, Bottom; }

  [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] struct INPUTUNION { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public INPUTUNION U; }

  static long Now() { return DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(); }

  /** Epoch ms when `pid` or one of its descendants (Firefox's launcher starts the real browser) first has a visible, non-empty top-level window; -1 after `timeoutMs`. */
  public static long WaitForWindow(int pid, int timeoutMs) {
    timeBeginPeriod(1);
    long end = Now() + timeoutMs;
    var parent = new Dictionary<long, long>();
    while (Now() < end) {
      bool found = false;
      EnumWindows((hwnd, l) => {
        uint owner;
        GetWindowThreadProcessId(hwnd, out owner);
        RECT r;
        if (IsWindowVisible(hwnd) && GetWindowRect(hwnd, out r) && r.Right - r.Left > 100 && r.Bottom - r.Top > 100) {
          if (owner != pid && !parent.ContainsKey(owner)) parent = Parents();
          if (Descends(owner, pid, parent)) { found = true; return false; }
        }
        return true;
      }, IntPtr.Zero);
      if (found) return Now();
      Thread.Sleep(1);
    }
    return -1;
  }

  /** Asks each visible window of `pid` and its descendants to close (WM_CLOSE, as its X button does); returns how many. */
  public static int Close(int pid) {
    var parent = Parents();
    int closed = 0;
    EnumWindows((hwnd, l) => {
      uint owner;
      GetWindowThreadProcessId(hwnd, out owner);
      if (IsWindowVisible(hwnd) && Descends(owner, pid, parent) && PostMessage(hwnd, 0x0010, IntPtr.Zero, IntPtr.Zero)) closed++;
      return true;
    }, IntPtr.Zero);
    return closed;
  }

  /** Sends one key chord like `ctrl+t` or `enter` through SendInput, as if typed; returns 'ok' or what went wrong. */
  public static string Key(string combo) {
    ushort vk = 0;
    bool ctrl = false, shift = false, alt = false;
    foreach (var part in combo.Split('+')) {
      if (part == "ctrl") ctrl = true;
      else if (part == "shift") shift = true;
      else if (part == "alt") alt = true;
      else vk = Vk(part);
    }
    if (vk == 0) return "unknown key " + combo;
    var inputs = new List<INPUT>();
    if (ctrl) AddKey(inputs, 0x11, true);
    if (shift) AddKey(inputs, 0x10, true);
    if (alt) AddKey(inputs, 0x12, true);
    AddKey(inputs, vk, true);
    AddKey(inputs, vk, false);
    if (alt) AddKey(inputs, 0x12, false);
    if (shift) AddKey(inputs, 0x10, false);
    if (ctrl) AddKey(inputs, 0x11, false);
    uint sent = SendInput((uint)inputs.Count, inputs.ToArray(), Marshal.SizeOf(typeof(INPUT)));
    return sent == (uint)inputs.Count ? "ok" : "sent " + sent + " of " + inputs.Count;
  }

  // PowerShell 5.1's Add-Type speaks C# 5: no local functions, so this one lives at class level.
  static void AddKey(List<INPUT> inputs, ushort vk, bool down) {
    var input = new INPUT();
    input.type = 1;
    input.U.ki = new KEYBDINPUT();
    input.U.ki.wVk = vk;
    input.U.ki.dwFlags = down ? 0u : 2u;
    inputs.Add(input);
  }

  static ushort Vk(string name) {
    switch (name) {
      case "tab": return 0x09;
      case "enter": return 0x0D;
      case "escape": return 0x1B;
      case "space": return 0x20;
    }
    if (name.Length == 1) {
      char c = char.ToUpperInvariant(name[0]);
      if ((c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z')) return (ushort)c;
    }
    return 0;
  }

  static bool Descends(long pid, long root, Dictionary<long, long> parent) {
    // Windows reuses ids, so a long chain is cut off.
    for (int depth = 0; depth < 8 && pid != root && parent.ContainsKey(pid) && pid != 0; depth++) pid = parent[pid];
    return pid == root;
  }

  static Dictionary<long, long> Parents() {
    var parent = new Dictionary<long, long>();
    Walk((p, pid, ppid) => parent[pid] = ppid);
    return parent;
  }

  /** Calls `row` with each process's entry in NtQuerySystemInformation(SystemProcessInformation). */
  static void Walk(Action<IntPtr, long, long> row) {
    int size = 1 << 22, needed;
    IntPtr buf = Marshal.AllocHGlobal(size);
    try {
      while (NtQuerySystemInformation(5, buf, size, out needed) == unchecked((int)0xC0000004)) {
        Marshal.FreeHGlobal(buf);
        size = needed + (1 << 20);
        buf = Marshal.AllocHGlobal(size);
      }
      int offset = 0;
      while (true) {
        IntPtr p = buf + offset;
        row(p, Marshal.ReadInt64(p, 80), Marshal.ReadInt64(p, 88));
        int next = Marshal.ReadInt32(p, 0);
        if (next == 0) break;
        offset += next;
      }
    } finally {
      Marshal.FreeHGlobal(buf);
    }
  }

  /** "pid:cpuMs:contextSwitches:privateBytes" for `root` and each of its descendants, joined by spaces. */
  public static string Stats(int root) {
    var parent = new Dictionary<long, long>();
    var rows = new Dictionary<long, long[]>();
    Walk((p, pid, ppid) => {
      int threads = Marshal.ReadInt32(p, 4);
      long privateWs = Marshal.ReadInt64(p, 8);
      long user = Marshal.ReadInt64(p, 40), kernel = Marshal.ReadInt64(p, 48);
      long switches = 0;
      for (int t = 0; t < threads; t++) switches += (uint)Marshal.ReadInt32(p, 256 + t * 80 + 64);
      parent[pid] = ppid;
      rows[pid] = new long[] { (user + kernel) / 10000, switches, privateWs };
    });
    var parts = new List<string>();
    foreach (var pid in rows.Keys) {
      if (!Descends(pid, root, parent)) continue;
      parts.Add(pid + ":" + rows[pid][0] + ":" + rows[pid][1] + ":" + rows[pid][2]);
    }
    return string.Join(" ", parts);
  }
}
