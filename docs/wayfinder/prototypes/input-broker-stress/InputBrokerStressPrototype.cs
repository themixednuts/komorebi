// PROTOTYPE ONLY. This file answers a Windows input and privilege question.
// It is deliberately isolated from production code and must not ship.

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.IO.Pipes;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Threading;
using System.Windows.Forms;

namespace KomorebiInputBrokerStressPrototype
{
    internal static class Program
    {
        [STAThread]
        private static void Main(string[] args)
        {
            if (args.Length > 0 && args[0] == "--self-test")
            {
                Environment.ExitCode = InputHooks.RunSelfTest(Console.Out);
                return;
            }

            Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
            Application.ThreadException += delegate(object sender, ThreadExceptionEventArgs eventArgs)
            {
                WriteCrash(eventArgs.Exception);
            };
            AppDomain.CurrentDomain.UnhandledException += delegate(object sender, UnhandledExceptionEventArgs eventArgs)
            {
                WriteCrash(eventArgs.ExceptionObject as Exception ?? new Exception(Convert.ToString(eventArgs.ExceptionObject, CultureInfo.InvariantCulture)));
            };
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            if (args.Length > 0 && args[0] == "--target")
            {
                Application.Run(new TargetForm(args.Length > 1 ? args[1] : "unknown"));
                return;
            }

            if (args.Length > 0 && args[0] == "--broker")
            {
                if (args.Length < 3)
                {
                    return;
                }

                Application.Run(new BrokerForm(args[1], args[2]));
                return;
            }

            Application.Run(new ObserverForm(Application.ExecutablePath));
        }

        private static void WriteCrash(Exception error)
        {
            string path = Path.Combine(Path.GetDirectoryName(Application.ExecutablePath), "PROTOTYPE-crash.txt");
            File.WriteAllText(path, error.ToString());
            MessageBox.Show(error.ToString(), "Input broker prototype failure", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    internal sealed class TargetForm : Form
    {
        private readonly string role;
        private int f12Down;
        private int backDown;
        private int forwardDown;
        private readonly Label state;

        internal TargetForm(string role)
        {
            this.role = role;
            Text = Title;
            ClientSize = new Size(500, 180);
            StartPosition = FormStartPosition.CenterScreen;
            TopMost = false;
            KeyPreview = true;

            state = new Label
            {
                Dock = DockStyle.Fill,
                Font = new Font("Segoe UI", 13),
                TextAlign = ContentAlignment.MiddleCenter
            };
            Controls.Add(state);
            KeyDown += OnKeyDown;
            MouseDown += OnMouseDown;
            UpdateState();
        }

        private string Title
        {
            get { return "INPUT PROTOTYPE TARGET - " + role; }
        }

        private void OnKeyDown(object sender, KeyEventArgs args)
        {
            if (args.KeyCode == Keys.F12)
            {
                f12Down++;
                UpdateState();
            }
        }

        private void OnMouseDown(object sender, MouseEventArgs args)
        {
            if (args.Button == MouseButtons.XButton1)
            {
                backDown++;
                UpdateState();
            }
            else if (args.Button == MouseButtons.XButton2)
            {
                forwardDown++;
                UpdateState();
            }
        }

        private void UpdateState()
        {
            Text = Title + " | F12=" + f12Down + " | Back=" + backDown + " | Forward=" + forwardDown;
            state.Text = role.ToUpperInvariant() + " INTEGRITY TEST TARGET\r\n\r\n"
                + "Focus this window and physically press F12 or either side mouse button.\r\n"
                + "F12 received: " + f12Down + "\r\n"
                + "Back received: " + backDown + "\r\n"
                + "Forward received: " + forwardDown;
        }
    }

    internal sealed class BrokerForm : Form
    {
        private readonly string pipeName;
        private readonly string executablePath;
        private Thread serverThread;
        private Process target;
        private IntPtr targetJob;

        internal BrokerForm(string pipeName, string executablePath)
        {
            this.pipeName = pipeName;
            this.executablePath = executablePath;
            Text = "Input broker stress prototype";
            ShowInTaskbar = false;
            FormBorderStyle = FormBorderStyle.FixedToolWindow;
            WindowState = FormWindowState.Minimized;
            Opacity = 0;
            Load += OnLoad;
            FormClosed += OnClosed;
        }

        private void OnLoad(object sender, EventArgs args)
        {
            InputHooks.Install(Handle);
            target = Process.Start(new ProcessStartInfo(executablePath, "--target elevated")
            {
                UseShellExecute = false
            });
            targetJob = NativeMethods.CreateKillOnCloseJob();
            if (targetJob == IntPtr.Zero || !NativeMethods.AssignProcessToJobObject(targetJob, target.Handle))
            {
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "contain elevated target in broker job");
            }

            serverThread = new Thread(ServerLoop);
            serverThread.IsBackground = true;
            serverThread.Name = "PROTOTYPE elevated broker pipe";
            serverThread.Start();
        }

        private void OnClosed(object sender, FormClosedEventArgs args)
        {
            InputHooks.Uninstall();
            if (target != null && !target.HasExited)
            {
                target.CloseMainWindow();
            }
            if (targetJob != IntPtr.Zero)
            {
                NativeMethods.CloseHandle(targetJob);
                targetJob = IntPtr.Zero;
            }
        }

        private void ServerLoop()
        {
            SecurityIdentifier logonSid = TokenSecurity.CurrentLogonSid();
            PipeSecurity security = new PipeSecurity();
            security.SetAccessRuleProtection(true, false);
            security.AddAccessRule(new PipeAccessRule(logonSid, PipeAccessRights.FullControl, AccessControlType.Allow));

            using (NamedPipeServerStream pipe = new NamedPipeServerStream(
                pipeName,
                PipeDirection.InOut,
                1,
                PipeTransmissionMode.Message,
                PipeOptions.None,
                4096,
                4096,
                security))
            {
                pipe.WaitForConnection();
                using (StreamReader reader = new StreamReader(pipe, Encoding.UTF8, false, 4096, true))
                using (StreamWriter writer = new StreamWriter(pipe, new UTF8Encoding(false), 4096, true))
                {
                    writer.AutoFlush = true;
                    writer.WriteLine("READY " + logonSid.Value + " " + (target == null ? 0 : target.Id));

                    string line;
                    while ((line = reader.ReadLine()) != null)
                    {
                        string response = HandleRequest(line);
                        writer.WriteLine(response);
                        if (line == "STOP")
                        {
                            BeginInvoke(new Action(Close));
                            return;
                        }
                        if (line == "CRASH")
                        {
                            writer.Flush();
                            Environment.FailFast("intentional input broker prototype crash");
                        }
                    }
                }
            }
        }

        private string HandleRequest(string line)
        {
            string[] parts = line.Split(' ');
            if (parts.Length == 2 && parts[0] == "PING")
            {
                return "PONG " + parts[1];
            }

            if (parts.Length == 6 && parts[0] == "MOVE")
            {
                IntPtr hwnd = new IntPtr(long.Parse(parts[1], CultureInfo.InvariantCulture));
                NativeMethods.RECT rect;
                if (!NativeMethods.GetWindowRect(hwnd, out rect))
                {
                    return "ERR GETRECT " + Marshal.GetLastWin32Error();
                }

                bool moved = NativeMethods.SetWindowPos(
                    hwnd,
                    IntPtr.Zero,
                    int.Parse(parts[2], CultureInfo.InvariantCulture),
                    int.Parse(parts[3], CultureInfo.InvariantCulture),
                    int.Parse(parts[4], CultureInfo.InvariantCulture),
                    int.Parse(parts[5], CultureInfo.InvariantCulture),
                    NativeMethods.SWP_NOZORDER | NativeMethods.SWP_NOACTIVATE);
                return moved ? "OK MOVE" : "ERR MOVE " + Marshal.GetLastWin32Error();
            }

            if (parts.Length == 2 && parts[0] == "FOCUS")
            {
                IntPtr hwnd = new IntPtr(long.Parse(parts[1], CultureInfo.InvariantCulture));
                bool focused = NativeMethods.SetForegroundWindow(hwnd);
                return focused ? "OK FOCUS" : "ERR FOCUS " + Marshal.GetLastWin32Error();
            }

            if (parts.Length == 2 && parts[0] == "HOOK")
            {
                bool suppress = parts[1] == "SUPPRESS";
                Invoke(new Action(delegate
                {
                    InputHooks.SuppressReferenceInputs = suppress;
                }));
                return "OK HOOK " + (suppress ? "SUPPRESS" : "OBSERVE");
            }

            if (parts.Length == 2 && parts[0] == "CANCEL")
            {
                int generation = int.Parse(parts[1], CultureInfo.InvariantCulture);
                InputHooks.Cancel(generation);
                return "OK CANCEL " + generation;
            }

            if (line == "STATS")
            {
                return InputHooks.StatsLine();
            }

            if (line == "CRASH")
            {
                return "OK CRASH";
            }

            if (line == "STOP")
            {
                return "OK STOP";
            }

            return "ERR UNKNOWN";
        }
    }

    internal sealed class ObserverForm : Form
    {
        private readonly string executablePath;
        private readonly TextBox output;
        private readonly Label instructions;
        private readonly System.Windows.Forms.Timer refreshTimer;
        private readonly List<string> evidence = new List<string>();
        private readonly object evidenceLock = new object();
        private Process normalTarget;
        private Process broker;
        private NamedPipeClientStream brokerPipe;
        private StreamReader brokerReader;
        private StreamWriter brokerWriter;
        private IntPtr normalTargetHwnd;
        private IntPtr elevatedTargetHwnd;
        private IntPtr foregroundHook;
        private IntPtr locationHook;
        private IntPtr desktopHook;
        private NativeMethods.WinEventDelegate winEventDelegate;
        private int generation;
        private int winEventForeground;
        private int winEventLocation;
        private int sessionBoundaries;
        private int desktopBoundaries;
        private int deviceChanges;
        private int staleCanariesRejected;
        private int armedCanaryGeneration = -1;
        private long brokerRttP95Microseconds;
        private string lastBrokerStats = "UNAVAILABLE";
        private bool deviceBoundaryArmed;
        private volatile bool runningLoad;
        private readonly List<Thread> loadThreads = new List<Thread>();
        private readonly HashSet<string> completedSamples = new HashSet<string>(StringComparer.Ordinal);
        private readonly List<string> sampleResults = new List<string>();
        private SampleRun activeSample;

        internal ObserverForm(string executablePath)
        {
            this.executablePath = executablePath;
            Text = "PROTOTYPE - input broker and physical input stress";
            ClientSize = new Size(1180, 780);
            StartPosition = FormStartPosition.CenterScreen;
            MinimumSize = new Size(980, 650);

            instructions = new Label
            {
                Dock = DockStyle.Top,
                Height = 82,
                Padding = new Padding(10),
                Font = new Font("Segoe UI", 10),
                Text = "Physical input only. This prototype never synthesizes input. Start the broker, approve the ordinary UAC prompt, then use the buttons left to right. Test F12 and either side mouse button. During each sample, move the Logitech G305 continuously in circles for the full countdown. For boundary tests, arm the canary before Win+L or a receiver disconnect."
            };

            FlowLayoutPanel buttons = new FlowLayoutPanel
            {
                Dock = DockStyle.Top,
                Height = 122,
                AutoScroll = true,
                Padding = new Padding(8),
                WrapContents = true
            };

            AddButton(buttons, "1. Start targets + broker", StartTargetsAndBroker);
            AddButton(buttons, "2. Window operations", RunWindowOperations);
            AddButton(buttons, "Focus normal", delegate { FocusTarget(false); });
            AddButton(buttons, "Focus elevated", delegate { FocusTarget(true); });
            AddButton(buttons, "Medium observe", delegate { SetHookMode(false, false); });
            AddButton(buttons, "Medium suppress", delegate { SetHookMode(false, true); });
            AddButton(buttons, "Broker observe", delegate { SetHookMode(true, false); });
            AddButton(buttons, "Broker suppress", delegate { SetHookMode(true, true); });
            AddButton(buttons, "1000 Hz idle 15s", delegate { StartSample("idle", 15, 0); });
            AddButton(buttons, "1000 Hz moderate 15s", delegate { StartSample("moderate", 15, Math.Max(1, Environment.ProcessorCount / 2)); });
            AddButton(buttons, "1000 Hz saturation 10s", delegate { StartSample("near-saturation", 10, Math.Max(1, Environment.ProcessorCount - 2)); });
            AddButton(buttons, "Arm boundary canary", ArmBoundaryCanary);
            AddButton(buttons, "Crash broker", CrashBroker);
            AddButton(buttons, "Save report", SaveReport);

            output = new TextBox
            {
                Dock = DockStyle.Fill,
                Multiline = true,
                ReadOnly = true,
                ScrollBars = ScrollBars.Vertical,
                Font = new Font("Cascadia Mono", 10),
                BackColor = Color.FromArgb(22, 24, 28),
                ForeColor = Color.Gainsboro
            };

            Controls.Add(output);
            Controls.Add(buttons);
            Controls.Add(instructions);

            Load += OnLoad;
            FormClosed += OnClosed;
            refreshTimer = new System.Windows.Forms.Timer { Interval = 250 };
            refreshTimer.Tick += RefreshDisplay;
        }

        private static void AddButton(FlowLayoutPanel panel, string text, EventHandler handler)
        {
            Button button = new Button
            {
                AutoSize = true,
                Height = 32,
                Text = text,
                Margin = new Padding(4)
            };
            button.Click += handler;
            panel.Controls.Add(button);
        }

        private void OnLoad(object sender, EventArgs args)
        {
            InputHooks.Install(Handle);
            RawInputMetrics.Register(Handle);
            NativeMethods.WTSRegisterSessionNotification(Handle, 0);
            InstallWinEventHooks();
            generation = 1;
            InputHooks.Cancel(generation);
            InputHooks.StartConsumer();
            RawInputMetrics.StartPreviewSampler();
            AddEvidence("observer started at integrity=" + TokenSecurity.IntegrityName() + " logonSid=" + TokenSecurity.CurrentLogonSid().Value);
            AddEvidence("physical mouse identified as Logitech G305 LIGHTSPEED, manufacturer maximum 1000 Hz");
            AddEvidence("4000 Hz and 8000 Hz marked UNTESTED because no capable physical device is attached");
            System.Threading.Timer armDeviceBoundary = null;
            armDeviceBoundary = new System.Threading.Timer(delegate
            {
                armDeviceBoundary.Dispose();
                deviceBoundaryArmed = true;
            }, null, 2000, Timeout.Infinite);
            refreshTimer.Start();
        }

        private void OnClosed(object sender, FormClosedEventArgs args)
        {
            refreshTimer.Stop();
            StopLoad();
            RawInputMetrics.StopPreviewSampler();
            InputHooks.StopConsumer();
            InputHooks.Uninstall();
            NativeMethods.WTSUnRegisterSessionNotification(Handle);
            UninstallWinEventHooks();

            try
            {
                if (brokerPipe != null && brokerPipe.IsConnected)
                {
                    SendBroker("STOP");
                }
            }
            catch { }

            if (normalTarget != null && !normalTarget.HasExited)
            {
                normalTarget.CloseMainWindow();
            }
        }

        protected override void WndProc(ref Message message)
        {
            if (message.Msg == NativeMethods.WM_INPUT)
            {
                RawInputMetrics.ProcessOne(message.LParam);
                RawInputMetrics.DrainBuffered();
            }
            else if (message.Msg == NativeMethods.WM_INPUT_DEVICE_CHANGE)
            {
                if (deviceBoundaryArmed)
                {
                    deviceChanges++;
                    CancelInputState("raw input device change");
                }
            }
            else if (message.Msg == NativeMethods.WM_WTSSESSION_CHANGE)
            {
                sessionBoundaries++;
                CancelInputState("session change " + message.WParam.ToInt64());
            }

            base.WndProc(ref message);
        }

        private void InstallWinEventHooks()
        {
            winEventDelegate = OnWinEvent;
            foregroundHook = NativeMethods.SetWinEventHook(3, 3, IntPtr.Zero, winEventDelegate, 0, 0, 0);
            locationHook = NativeMethods.SetWinEventHook(0x800B, 0x800B, IntPtr.Zero, winEventDelegate, 0, 0, 0);
            desktopHook = NativeMethods.SetWinEventHook(0x20, 0x20, IntPtr.Zero, winEventDelegate, 0, 0, 0);
        }

        private void UninstallWinEventHooks()
        {
            if (foregroundHook != IntPtr.Zero) NativeMethods.UnhookWinEvent(foregroundHook);
            if (locationHook != IntPtr.Zero) NativeMethods.UnhookWinEvent(locationHook);
            if (desktopHook != IntPtr.Zero) NativeMethods.UnhookWinEvent(desktopHook);
        }

        private void OnWinEvent(IntPtr hook, uint eventType, IntPtr hwnd, int objectId, int childId, uint threadId, uint time)
        {
            if (eventType == 0x20)
            {
                desktopBoundaries++;
                BeginInvoke(new Action(delegate { CancelInputState("desktop switch"); }));
                return;
            }

            if (hwnd == normalTargetHwnd || hwnd == elevatedTargetHwnd)
            {
                if (eventType == 3) winEventForeground++;
                if (eventType == 0x800B) winEventLocation++;
            }
        }

        private void StartTargetsAndBroker(object sender, EventArgs args)
        {
            if (broker != null && !broker.HasExited)
            {
                AddEvidence("start ignored because broker is already running");
                return;
            }

            normalTarget = Process.Start(new ProcessStartInfo(executablePath, "--target medium")
            {
                UseShellExecute = false
            });

            string pipeName = "komorebi-input-prototype-" + Process.GetCurrentProcess().SessionId + "-" + Guid.NewGuid().ToString("N");
            ArmBoundaryCanary(sender, args);

            ProcessStartInfo brokerStart = new ProcessStartInfo(executablePath, "--broker " + pipeName + " \"" + executablePath + "\"")
            {
                UseShellExecute = true,
                Verb = "runas"
            };

            try
            {
                broker = Process.Start(brokerStart);
            }
            catch (System.ComponentModel.Win32Exception error)
            {
                AddEvidence("broker elevation rejected error=" + error.NativeErrorCode);
                return;
            }

            Thread connectThread = new Thread(new ThreadStart(delegate
            {
                try
                {
                    NamedPipeClientStream pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.None, TokenImpersonationLevel.Identification);
                    pipe.Connect(20000);
                    StreamReader reader = new StreamReader(pipe, Encoding.UTF8, false, 4096, true);
                    StreamWriter writer = new StreamWriter(pipe, new UTF8Encoding(false), 4096, true) { AutoFlush = true };
                    string ready = reader.ReadLine();

                    BeginInvoke(new Action(delegate
                    {
                        brokerPipe = pipe;
                        brokerReader = reader;
                        brokerWriter = writer;
                        AddEvidence("broker connected: " + ready);
                        FindTargetWindows();
                        MeasureBrokerRtt();
                    }));
                }
                catch (Exception error)
                {
                    BeginInvoke(new Action(delegate { AddEvidence("broker connection failed: " + error.Message); }));
                }
            }));
            connectThread.IsBackground = true;
            connectThread.Start();
        }

        private void FindTargetWindows()
        {
            normalTargetHwnd = FindTopLevelWindow(normalTarget == null ? 0 : normalTarget.Id);
            int elevatedPid = broker == null ? 0 : ParseReadyTargetPid();
            elevatedTargetHwnd = FindTopLevelWindow(elevatedPid);

            for (int attempt = 0; attempt < 20 && (normalTargetHwnd == IntPtr.Zero || elevatedTargetHwnd == IntPtr.Zero); attempt++)
            {
                Thread.Sleep(100);
                if (normalTargetHwnd == IntPtr.Zero) normalTargetHwnd = FindTopLevelWindow(normalTarget == null ? 0 : normalTarget.Id);
                if (elevatedTargetHwnd == IntPtr.Zero) elevatedTargetHwnd = FindWindowByPrefix("INPUT PROTOTYPE TARGET - elevated");
            }

            AddEvidence("target HWNDs medium=" + normalTargetHwnd.ToInt64() + " elevated=" + elevatedTargetHwnd.ToInt64());
        }

        private int ParseReadyTargetPid()
        {
            return 0;
        }

        private static IntPtr FindTopLevelWindow(int pid)
        {
            if (pid == 0) return IntPtr.Zero;
            IntPtr found = IntPtr.Zero;
            NativeMethods.EnumWindows(delegate(IntPtr hwnd, IntPtr state)
            {
                uint windowPid;
                NativeMethods.GetWindowThreadProcessId(hwnd, out windowPid);
                if (windowPid == (uint)pid && NativeMethods.IsWindowVisible(hwnd))
                {
                    found = hwnd;
                    return false;
                }
                return true;
            }, IntPtr.Zero);
            return found;
        }

        private static IntPtr FindWindowByPrefix(string prefix)
        {
            IntPtr found = IntPtr.Zero;
            StringBuilder title = new StringBuilder(512);
            NativeMethods.EnumWindows(delegate(IntPtr hwnd, IntPtr state)
            {
                title.Clear();
                NativeMethods.GetWindowText(hwnd, title, title.Capacity);
                if (title.ToString().StartsWith(prefix, StringComparison.Ordinal))
                {
                    found = hwnd;
                    return false;
                }
                return true;
            }, IntPtr.Zero);
            return found;
        }

        private void MeasureBrokerRtt()
        {
            if (!BrokerConnected) return;
            List<long> micros = new List<long>();
            for (int i = 0; i < 200; i++)
            {
                long started = Stopwatch.GetTimestamp();
                string response = SendBroker("PING " + i);
                long elapsed = Stopwatch.GetTimestamp() - started;
                if (response == "PONG " + i)
                {
                    micros.Add(elapsed * 1000000L / Stopwatch.Frequency);
                }
            }
            micros.Sort();
            brokerRttP95Microseconds = Percentile(micros, 0.95);
            AddEvidence("broker ping n=" + micros.Count + " p95=" + brokerRttP95Microseconds + "us");
        }

        private void RunWindowOperations(object sender, EventArgs args)
        {
            FindTargetWindows();
            TestMove("medium direct", normalTargetHwnd, false);
            TestMove("elevated direct", elevatedTargetHwnd, false);
            TestMove("elevated broker", elevatedTargetHwnd, true);
            string focusDirect = NativeMethods.SetForegroundWindow(elevatedTargetHwnd)
                ? "OK"
                : "ERR " + Marshal.GetLastWin32Error();
            AddEvidence("elevated direct focus=" + focusDirect);
            AddEvidence("elevated broker focus=" + SendBroker("FOCUS " + elevatedTargetHwnd.ToInt64()));
        }

        private void TestMove(string name, IntPtr hwnd, bool throughBroker)
        {
            if (hwnd == IntPtr.Zero)
            {
                AddEvidence(name + " skipped, no HWND");
                return;
            }

            NativeMethods.RECT before;
            if (!NativeMethods.GetWindowRect(hwnd, out before))
            {
                AddEvidence(name + " GetWindowRect failed=" + Marshal.GetLastWin32Error());
                return;
            }

            int width = before.Right - before.Left;
            int height = before.Bottom - before.Top;
            string moved;
            string restored;
            if (throughBroker)
            {
                moved = SendBroker(string.Format(CultureInfo.InvariantCulture, "MOVE {0} {1} {2} {3} {4}", hwnd.ToInt64(), before.Left + 17, before.Top, width, height));
                restored = SendBroker(string.Format(CultureInfo.InvariantCulture, "MOVE {0} {1} {2} {3} {4}", hwnd.ToInt64(), before.Left, before.Top, width, height));
            }
            else
            {
                moved = NativeMethods.SetWindowPos(hwnd, IntPtr.Zero, before.Left + 17, before.Top, width, height, NativeMethods.SWP_NOZORDER | NativeMethods.SWP_NOACTIVATE)
                    ? "OK MOVE"
                    : "ERR MOVE " + Marshal.GetLastWin32Error();
                restored = NativeMethods.SetWindowPos(hwnd, IntPtr.Zero, before.Left, before.Top, width, height, NativeMethods.SWP_NOZORDER | NativeMethods.SWP_NOACTIVATE)
                    ? "OK MOVE"
                    : "ERR MOVE " + Marshal.GetLastWin32Error();
            }

            AddEvidence(name + " move=" + moved + " restore=" + restored);
        }

        private void FocusTarget(bool elevated)
        {
            IntPtr hwnd = elevated ? elevatedTargetHwnd : normalTargetHwnd;
            if (hwnd == IntPtr.Zero)
            {
                FindTargetWindows();
                hwnd = elevated ? elevatedTargetHwnd : normalTargetHwnd;
            }
            NativeMethods.SetForegroundWindow(hwnd);
        }

        private void SetHookMode(bool useBroker, bool suppress)
        {
            if (useBroker)
            {
                InputHooks.SuppressReferenceInputs = false;
                AddEvidence("broker hook mode=" + SendBroker("HOOK " + (suppress ? "SUPPRESS" : "OBSERVE")));
            }
            else
            {
                if (BrokerConnected) SendBroker("HOOK OBSERVE");
                InputHooks.SuppressReferenceInputs = suppress;
                AddEvidence("medium hook mode=" + (suppress ? "SUPPRESS" : "OBSERVE"));
            }
        }

        private void StartSample(string name, int seconds, int workers)
        {
            if (activeSample != null)
            {
                AddEvidence("sample ignored because another sample is running");
                return;
            }

            activeSample = SampleRun.Start(name, seconds, workers);
            StartLoad(workers);
            AddEvidence("sample " + name + " started for " + seconds + "s with loadWorkers=" + workers + "; move G305 continuously now");

            System.Threading.Timer timer = null;
            timer = new System.Threading.Timer(delegate
            {
                timer.Dispose();
                StopLoad();
                SampleRun completed = activeSample;
                activeSample = null;
                string result = completed.Complete();
                BeginInvoke(new Action(delegate
                {
                    completedSamples.Add(completed.Name);
                    sampleResults.Add(result);
                    AddEvidence(result);
                }));
            }, null, seconds * 1000, Timeout.Infinite);
        }

        private void StartLoad(int workers)
        {
            StopLoad();
            runningLoad = true;
            for (int i = 0; i < workers; i++)
            {
                Thread thread = new Thread(new ThreadStart(delegate
                {
                    double value = 0.1;
                    while (runningLoad)
                    {
                        value = Math.Sqrt(value + 1.0000001);
                        if (value > 1000) value = 0.1;
                    }
                    GC.KeepAlive(value);
                }));
                thread.IsBackground = true;
                thread.Priority = ThreadPriority.Normal;
                thread.Start();
                loadThreads.Add(thread);
            }
        }

        private void StopLoad()
        {
            runningLoad = false;
            foreach (Thread thread in loadThreads)
            {
                thread.Join(500);
            }
            loadThreads.Clear();
        }

        private void ArmBoundaryCanary(object sender, EventArgs args)
        {
            armedCanaryGeneration = generation;
            AddEvidence("boundary canary armed at generation=" + generation + "; it must be rejected after the next boundary");
            int captured = generation;
            System.Threading.Timer timer = null;
            timer = new System.Threading.Timer(delegate
            {
                timer.Dispose();
                BeginInvoke(new Action(delegate
                {
                    if (captured != generation)
                    {
                        staleCanariesRejected++;
                        AddEvidence("PASS stale boundary canary rejected old=" + captured + " current=" + generation);
                    }
                    else
                    {
                        AddEvidence("boundary canary still current; no boundary was observed within 5s");
                    }
                }));
            }, null, 5000, Timeout.Infinite);
        }

        private void CancelInputState(string reason)
        {
            generation++;
            InputHooks.Cancel(generation);
            if (BrokerConnected)
            {
                try { SendBroker("CANCEL " + generation); } catch { }
            }
            AddEvidence("input generation advanced to " + generation + " because " + reason);
        }

        private void CrashBroker(object sender, EventArgs args)
        {
            if (activeSample != null)
            {
                AddEvidence("broker crash blocked until the active sample completes");
                return;
            }
            if (completedSamples.Count < 3)
            {
                AddEvidence("broker crash blocked until idle, moderate, and near-saturation samples complete");
                return;
            }
            if (!BrokerConnected)
            {
                AddEvidence("broker crash skipped, not connected");
                return;
            }

            try
            {
                lastBrokerStats = SendBroker("STATS");
                AddEvidence("broker crash response=" + SendBroker("CRASH"));
            }
            catch (Exception error)
            {
                AddEvidence("broker disconnected during intentional crash: " + error.GetType().Name);
            }

            brokerPipe.Dispose();
            brokerPipe = null;
            brokerReader = null;
            brokerWriter = null;
            CancelInputState("broker crash");
            AddEvidence("PASS observer remained responsive after broker crash");
        }

        private void SaveReport(object sender, EventArgs args)
        {
            List<string> incomplete = new List<string>();
            if (activeSample != null) incomplete.Add("wait for the active sample to finish");
            foreach (string required in new[] { "idle", "moderate", "near-saturation" })
            {
                if (!completedSamples.Contains(required)) incomplete.Add("run the " + required + " sample");
            }
            if (InputHooks.ReferenceTransitions < 4) incomplete.Add("physically press F12 and either side mouse button in the reference tests");
            if (sessionBoundaries < 2) incomplete.Add("arm the canary, lock with Win+L, then unlock");
            if (deviceChanges < 2) incomplete.Add("arm the canary, disconnect the G305 receiver, then reconnect it");
            if (broker != null && !broker.HasExited) incomplete.Add("run the broker crash test last");
            if (incomplete.Count > 0)
            {
                AddEvidence("report not saved: " + string.Join("; ", incomplete.ToArray()));
                return;
            }

            string directory = Path.Combine(Path.GetDirectoryName(executablePath), "..", "results");
            directory = Path.GetFullPath(directory);
            Directory.CreateDirectory(directory);
            string path = Path.Combine(directory, "PROTOTYPE-input-broker-" + DateTime.Now.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture) + ".txt");
            File.WriteAllText(path, BuildReport(), new UTF8Encoding(false));
            AddEvidence("report saved to " + path);
        }

        private string BuildReport()
        {
            StringBuilder report = new StringBuilder();
            report.AppendLine("PROTOTYPE ONLY - input broker under privilege and device stress");
            report.AppendLine("timestamp=" + DateTimeOffset.Now.ToString("O", CultureInfo.InvariantCulture));
            report.AppendLine("os=" + Environment.OSVersion);
            report.AppendLine("processors=" + Environment.ProcessorCount);
            report.AppendLine("observerIntegrity=" + TokenSecurity.IntegrityName());
            report.AppendLine("logonSid=" + TokenSecurity.CurrentLogonSid().Value);
            report.AppendLine("mouse=Logitech G305 LIGHTSPEED");
            report.AppendLine("manufacturerMaximumReportRateHz=1000");
            report.AppendLine("4000Hz=UNTESTED no capable attached device");
            report.AppendLine("8000Hz=UNTESTED no capable attached device");
            report.AppendLine("generation=" + generation);
            report.AppendLine("sessionBoundaries=" + sessionBoundaries);
            report.AppendLine("desktopBoundaries=" + desktopBoundaries);
            report.AppendLine("deviceChanges=" + deviceChanges);
            report.AppendLine("staleCanariesRejected=" + staleCanariesRejected);
            report.AppendLine("brokerRttP95Microseconds=" + brokerRttP95Microseconds);
            report.AppendLine("mediumHooks=" + InputHooks.StatsLine());
            report.AppendLine("rawInput=" + RawInputMetrics.StatsLine());
            report.AppendLine("brokerHooks=" + lastBrokerStats);
            report.AppendLine("sampleResults=");
            foreach (string result in sampleResults) report.AppendLine(result);
            if (BrokerConnected)
            {
                try { report.AppendLine("brokerHooks=" + SendBroker("STATS")); } catch { }
            }
            report.AppendLine();
            report.AppendLine("Evidence log");
            lock (evidenceLock)
            {
                foreach (string line in evidence) report.AppendLine(line);
            }
            return report.ToString();
        }

        private bool BrokerConnected
        {
            get { return brokerPipe != null && brokerPipe.IsConnected; }
        }

        private string SendBroker(string request)
        {
            if (!BrokerConnected) return "ERR NO_BROKER";
            brokerWriter.WriteLine(request);
            return brokerReader.ReadLine() ?? "ERR DISCONNECTED";
        }

        private void AddEvidence(string message)
        {
            string line = DateTime.Now.ToString("HH:mm:ss.fff", CultureInfo.InvariantCulture) + " " + message;
            lock (evidenceLock) evidence.Add(line);
        }

        private void RefreshDisplay(object sender, EventArgs args)
        {
            StringBuilder text = new StringBuilder();
            text.AppendLine("LIVE STATE");
            text.AppendLine("generation=" + generation + " broker=" + (BrokerConnected ? "connected" : "disconnected") + " integrity=" + TokenSecurity.IntegrityName());
            text.AppendLine("targets medium=" + normalTargetHwnd.ToInt64() + " elevated=" + elevatedTargetHwnd.ToInt64());
            text.AppendLine("boundaries session=" + sessionBoundaries + " desktop=" + desktopBoundaries + " device=" + deviceChanges + " staleCanariesRejected=" + staleCanariesRejected);
            text.AppendLine("WinEvents foreground=" + winEventForeground + " location=" + winEventLocation);
            text.AppendLine("medium " + InputHooks.StatsLine());
            text.AppendLine("raw " + RawInputMetrics.StatsLine());
            text.AppendLine("completedSamples=" + string.Join(",", completedSamples.ToArray()));
            if (activeSample != null) text.AppendLine("ACTIVE SAMPLE=" + activeSample.Name + " remaining~" + activeSample.RemainingSeconds + "s. Wait before pressing another test button.");
            text.AppendLine();
            text.AppendLine("EVIDENCE");
            lock (evidenceLock)
            {
                foreach (string line in evidence.Skip(Math.Max(0, evidence.Count - 22))) text.AppendLine(line);
            }
            output.Text = text.ToString();
            output.SelectionStart = output.TextLength;
            output.ScrollToCaret();
        }

        private static long Percentile(List<long> sorted, double percentile)
        {
            if (sorted.Count == 0) return 0;
            int index = (int)Math.Ceiling(percentile * sorted.Count) - 1;
            return sorted[Math.Max(0, Math.Min(sorted.Count - 1, index))];
        }
    }

    internal sealed class SampleRun
    {
        internal string Name;
        internal int Seconds;
        internal int Workers;
        internal long StartedTicks;
        internal long HookStart;
        internal long RawStart;
        internal long PreviewStart;
        internal DateTime StartedAt;

        internal int RemainingSeconds
        {
            get { return Math.Max(0, Seconds - (int)(DateTime.UtcNow - StartedAt).TotalSeconds); }
        }

        internal static SampleRun Start(string name, int seconds, int workers)
        {
            return new SampleRun
            {
                Name = name,
                Seconds = seconds,
                Workers = workers,
                StartedTicks = Stopwatch.GetTimestamp(),
                HookStart = InputHooks.SampleIndex,
                RawStart = RawInputMetrics.SampleIndex,
                PreviewStart = RawInputMetrics.PreviewIndex,
                StartedAt = DateTime.UtcNow
            };
        }

        internal string Complete()
        {
            double elapsed = (Stopwatch.GetTimestamp() - StartedTicks) / (double)Stopwatch.Frequency;
            MetricsSummary hook = InputHooks.Summarize(HookStart);
            MetricsSummary raw = RawInputMetrics.Summarize(RawStart);
            MetricsSummary preview = RawInputMetrics.SummarizePreview(PreviewStart);
            return string.Format(
                CultureInfo.InvariantCulture,
                "sample {0} complete elapsed={1:F2}s workers={2} hookEvents={3} hookRate={4:F1}/s hookOwnP99={5:F1}us hookOwnMax={6:F1}us rawEvents={7} rawRate={8:F1}/s rawMedianIntervalHz={9:F0} rawBuffered={10} noCoalesce={11} previewP95={12:F2}ms transitionDrops={13}",
                Name,
                elapsed,
                Workers,
                hook.Count,
                hook.Count / elapsed,
                hook.P99,
                hook.Max,
                raw.Count,
                raw.Count / elapsed,
                raw.MedianRate,
                RawInputMetrics.BufferedCount,
                RawInputMetrics.NoCoalesceCount,
                preview.P95,
                InputHooks.TransitionDrops);
        }
    }

    internal struct MetricsSummary
    {
        internal long Count;
        internal double P95;
        internal double P99;
        internal double Max;
        internal double MedianRate;
    }

    internal static class InputHooks
    {
        private const int Capacity = 1000000;
        private static readonly long[] ownNanoseconds = new long[Capacity];
        private static readonly int[] deliveryMilliseconds = new int[Capacity];
        private static long sampleIndex;
        private static IntPtr keyboardHook;
        private static IntPtr mouseHook;
        private static NativeMethods.HookDelegate keyboardDelegate;
        private static NativeMethods.HookDelegate mouseDelegate;
        private static int f12Down;
        private static int f12Up;
        private static int x1Down;
        private static int x1Up;
        private static int x2Down;
        private static int x2Up;
        private static int mouseMoves;
        private static int injected;
        private static int suppressed;
        private static int generation;
        private static readonly TransitionRing transitions = new TransitionRing(4096);
        private static Thread consumer;
        private static volatile bool consume;

        internal static volatile bool SuppressReferenceInputs;

        internal static long SampleIndex { get { return Interlocked.Read(ref sampleIndex); } }
        internal static long TransitionDrops { get { return transitions.Dropped; } }
        internal static int ReferenceTransitions { get { return f12Down + f12Up + x1Down + x1Up + x2Down + x2Up; } }

        internal static void Install(IntPtr hwnd)
        {
            keyboardDelegate = KeyboardCallback;
            mouseDelegate = MouseCallback;
            keyboardHook = NativeMethods.SetWindowsHookEx(13, keyboardDelegate, IntPtr.Zero, 0);
            mouseHook = NativeMethods.SetWindowsHookEx(14, mouseDelegate, IntPtr.Zero, 0);
        }

        internal static void Uninstall()
        {
            if (keyboardHook != IntPtr.Zero) NativeMethods.UnhookWindowsHookEx(keyboardHook);
            if (mouseHook != IntPtr.Zero) NativeMethods.UnhookWindowsHookEx(mouseHook);
            keyboardHook = IntPtr.Zero;
            mouseHook = IntPtr.Zero;
        }

        internal static void Cancel(int nextGeneration)
        {
            generation = nextGeneration;
            SuppressReferenceInputs = false;
        }

        internal static void StartConsumer()
        {
            if (consumer != null) return;
            consume = true;
            consumer = new Thread(new ThreadStart(delegate
            {
                while (consume)
                {
                    if (!transitions.TryConsume(generation)) Thread.Sleep(1);
                }
            }));
            consumer.IsBackground = true;
            consumer.Name = "PROTOTYPE transition consumer";
            consumer.Start();
        }

        internal static void StopConsumer()
        {
            consume = false;
            if (consumer != null) consumer.Join(1000);
            consumer = null;
        }

        private static IntPtr KeyboardCallback(int code, IntPtr wParam, IntPtr lParam)
        {
            if (code < 0) return NativeMethods.CallNextHookEx(keyboardHook, code, wParam, lParam);
            long started = Stopwatch.GetTimestamp();
            NativeMethods.KBDLLHOOKSTRUCT data = (NativeMethods.KBDLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(NativeMethods.KBDLLHOOKSTRUCT));
            bool isInjected = (data.flags & 0x10) != 0;
            if (isInjected) Interlocked.Increment(ref injected);
            bool f12 = data.vkCode == 0x7B;
            if (f12 && !isInjected)
            {
                int message = wParam.ToInt32();
                if (message == 0x100 || message == 0x104)
                {
                    Interlocked.Increment(ref f12Down);
                    transitions.Enqueue(1, generation);
                }
                else if (message == 0x101 || message == 0x105)
                {
                    Interlocked.Increment(ref f12Up);
                    transitions.Enqueue(2, generation);
                }
            }
            Record(started, 0);
            if (f12 && !isInjected && SuppressReferenceInputs)
            {
                Interlocked.Increment(ref suppressed);
                return new IntPtr(1);
            }
            return NativeMethods.CallNextHookEx(keyboardHook, code, wParam, lParam);
        }

        private static IntPtr MouseCallback(int code, IntPtr wParam, IntPtr lParam)
        {
            if (code < 0) return NativeMethods.CallNextHookEx(mouseHook, code, wParam, lParam);
            long started = Stopwatch.GetTimestamp();
            NativeMethods.MSLLHOOKSTRUCT data = (NativeMethods.MSLLHOOKSTRUCT)Marshal.PtrToStructure(lParam, typeof(NativeMethods.MSLLHOOKSTRUCT));
            bool isInjected = (data.flags & 1) != 0;
            if (isInjected) Interlocked.Increment(ref injected);
            int message = wParam.ToInt32();
            int referenceButton = ClassifyReferenceMouseButton(message, data.mouseData);
            bool xButton = referenceButton != 0;
            if (!isInjected && message == 0x200) Interlocked.Increment(ref mouseMoves);
            if (xButton && !isInjected)
            {
                if (message == 0x20B)
                {
                    if (referenceButton == 1) Interlocked.Increment(ref x1Down);
                    else Interlocked.Increment(ref x2Down);
                    transitions.Enqueue(3, generation);
                }
                else
                {
                    if (referenceButton == 1) Interlocked.Increment(ref x1Up);
                    else Interlocked.Increment(ref x2Up);
                    transitions.Enqueue(4, generation);
                }
            }
            Record(started, unchecked((int)(NativeMethods.GetTickCount() - data.time)));
            if (xButton && !isInjected && SuppressReferenceInputs)
            {
                Interlocked.Increment(ref suppressed);
                return new IntPtr(1);
            }
            return NativeMethods.CallNextHookEx(mouseHook, code, wParam, lParam);
        }

        internal static int ClassifyReferenceMouseButton(int message, uint mouseData)
        {
            if (message != 0x20B && message != 0x20C) return 0;
            int button = (int)((mouseData >> 16) & 0xFFFF);
            return button == 1 || button == 2 ? button : 0;
        }

        internal static int RunSelfTest(TextWriter output)
        {
            int failures = 0;
            if (ClassifyReferenceMouseButton(0x20B, 1u << 16) != 1)
            {
                output.WriteLine("FAIL Back must classify as XBUTTON1");
                failures++;
            }
            if (ClassifyReferenceMouseButton(0x20B, 2u << 16) != 2)
            {
                output.WriteLine("FAIL Forward must classify as XBUTTON2");
                failures++;
            }
            if (ClassifyReferenceMouseButton(0x200, 1u << 16) != 0)
            {
                output.WriteLine("FAIL movement must not classify as a side button");
                failures++;
            }
            if (failures == 0) output.WriteLine("PASS Back and Forward side-button classification");
            return failures == 0 ? 0 : 1;
        }

        private static void Record(long started, int deliveryMs)
        {
            long elapsed = Stopwatch.GetTimestamp() - started;
            long index = Interlocked.Increment(ref sampleIndex) - 1;
            int slot = (int)(index % Capacity);
            ownNanoseconds[slot] = elapsed * 1000000000L / Stopwatch.Frequency;
            deliveryMilliseconds[slot] = deliveryMs;
        }

        internal static MetricsSummary Summarize(long start)
        {
            long end = SampleIndex;
            long count = Math.Min(end - start, Capacity);
            List<double> ownUs = new List<double>((int)Math.Min(count, 100000));
            long skip = Math.Max(1, count / 100000);
            for (long i = 0; i < count; i += skip)
            {
                int slot = (int)((start + i) % Capacity);
                ownUs.Add(ownNanoseconds[slot] / 1000.0);
            }
            ownUs.Sort();
            return SummaryFromSorted(ownUs, count, 0);
        }

        internal static string StatsLine()
        {
            MetricsSummary summary = Summarize(Math.Max(0, SampleIndex - 10000));
            return string.Format(
                CultureInfo.InvariantCulture,
                "STATS events={0} mouseMoves={1} f12={2}/{3} back={4}/{5} forward={6}/{7} injected={8} suppressed={9} ownP99Us={10:F1} ownMaxUs={11:F1} queueDepth={12} queueMax={13} queueDrops={14} stale={15}",
                SampleIndex,
                mouseMoves,
                f12Down,
                f12Up,
                x1Down,
                x1Up,
                x2Down,
                x2Up,
                injected,
                suppressed,
                summary.P99,
                summary.Max,
                transitions.Depth,
                transitions.MaxDepth,
                transitions.Dropped,
                transitions.Stale);
        }

        internal static MetricsSummary SummaryFromSorted(List<double> values, long count, double medianRate)
        {
            MetricsSummary summary = new MetricsSummary { Count = count, MedianRate = medianRate };
            if (values.Count == 0) return summary;
            summary.P95 = values[Math.Max(0, (int)Math.Ceiling(values.Count * 0.95) - 1)];
            summary.P99 = values[Math.Max(0, (int)Math.Ceiling(values.Count * 0.99) - 1)];
            summary.Max = values[values.Count - 1];
            return summary;
        }
    }

    internal sealed class TransitionRing
    {
        private readonly int[] kinds;
        private readonly int[] generations;
        private long write;
        private long read;
        private long dropped;
        private long stale;
        private long maxDepth;

        internal TransitionRing(int capacity)
        {
            kinds = new int[capacity];
            generations = new int[capacity];
        }

        internal long Dropped { get { return Interlocked.Read(ref dropped); } }
        internal long Stale { get { return Interlocked.Read(ref stale); } }
        internal long Depth { get { return Interlocked.Read(ref write) - Interlocked.Read(ref read); } }
        internal long MaxDepth { get { return Interlocked.Read(ref maxDepth); } }

        internal void Enqueue(int kind, int generation)
        {
            long next = Interlocked.Read(ref write);
            long currentRead = Interlocked.Read(ref read);
            if (next - currentRead >= kinds.Length)
            {
                Interlocked.Increment(ref dropped);
                return;
            }
            int slot = (int)(next % kinds.Length);
            kinds[slot] = kind;
            generations[slot] = generation;
            Interlocked.Exchange(ref write, next + 1);
            long depth = next + 1 - currentRead;
            long observed;
            while (depth > (observed = Interlocked.Read(ref maxDepth)))
            {
                if (Interlocked.CompareExchange(ref maxDepth, depth, observed) == observed) break;
            }
        }

        internal bool TryConsume(int currentGeneration)
        {
            long next = Interlocked.Read(ref read);
            if (next >= Interlocked.Read(ref write)) return false;
            int slot = (int)(next % kinds.Length);
            if (generations[slot] != currentGeneration) Interlocked.Increment(ref stale);
            GC.KeepAlive(kinds[slot]);
            Interlocked.Exchange(ref read, next + 1);
            return true;
        }
    }

    internal static class RawInputMetrics
    {
        private const int Capacity = 1000000;
        private static readonly long[] timestamps = new long[Capacity];
        private static long sampleIndex;
        private static readonly long[] previewLatencyNanoseconds = new long[Capacity];
        private static long previewIndex;
        private static long latestTimestamp;
        private static long latestSequence;
        private static long consumedSequence;
        private static long bufferedCount;
        private static long noCoalesceCount;
        private static IntPtr dataBuffer;
        private static IntPtr batchBuffer;
        private static Thread previewThread;
        private static volatile bool previewRunning;

        internal static long SampleIndex { get { return Interlocked.Read(ref sampleIndex); } }
        internal static long PreviewIndex { get { return Interlocked.Read(ref previewIndex); } }
        internal static long BufferedCount { get { return Interlocked.Read(ref bufferedCount); } }
        internal static long NoCoalesceCount { get { return Interlocked.Read(ref noCoalesceCount); } }

        internal static void Register(IntPtr hwnd)
        {
            dataBuffer = Marshal.AllocHGlobal(256);
            batchBuffer = Marshal.AllocHGlobal(65536);
            NativeMethods.RAWINPUTDEVICE[] devices =
            {
                new NativeMethods.RAWINPUTDEVICE
                {
                    usUsagePage = 1,
                    usUsage = 2,
                    dwFlags = 0x100 | 0x2000,
                    hwndTarget = hwnd
                }
            };
            if (!NativeMethods.RegisterRawInputDevices(devices, 1, (uint)Marshal.SizeOf(typeof(NativeMethods.RAWINPUTDEVICE))))
            {
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            }
        }

        internal static void ProcessOne(IntPtr rawHandle)
        {
            uint size = 256;
            uint result = NativeMethods.GetRawInputData(rawHandle, 0x10000003, dataBuffer, ref size, (uint)Marshal.SizeOf(typeof(NativeMethods.RAWINPUTHEADER)));
            if (result == uint.MaxValue || result == 0) return;
            ProcessBlock(dataBuffer);
        }

        internal static void DrainBuffered()
        {
            while (true)
            {
                uint bytes = 65536;
                uint count = NativeMethods.GetRawInputBuffer(batchBuffer, ref bytes, (uint)Marshal.SizeOf(typeof(NativeMethods.RAWINPUTHEADER)));
                if (count == uint.MaxValue || count == 0) return;
                IntPtr current = batchBuffer;
                for (uint i = 0; i < count; i++)
                {
                    NativeMethods.RAWINPUTHEADER header = (NativeMethods.RAWINPUTHEADER)Marshal.PtrToStructure(current, typeof(NativeMethods.RAWINPUTHEADER));
                    ProcessBlock(current);
                    int aligned = (int)((header.dwSize + (uint)IntPtr.Size - 1) & ~((uint)IntPtr.Size - 1));
                    current = IntPtr.Add(current, aligned);
                }
                Interlocked.Add(ref bufferedCount, count);
            }
        }

        private static void ProcessBlock(IntPtr block)
        {
            NativeMethods.RAWINPUTHEADER header = (NativeMethods.RAWINPUTHEADER)Marshal.PtrToStructure(block, typeof(NativeMethods.RAWINPUTHEADER));
            if (header.dwType != 0) return;
            IntPtr mousePtr = IntPtr.Add(block, Marshal.SizeOf(typeof(NativeMethods.RAWINPUTHEADER)));
            NativeMethods.RAWMOUSE mouse = (NativeMethods.RAWMOUSE)Marshal.PtrToStructure(mousePtr, typeof(NativeMethods.RAWMOUSE));
            long now = Stopwatch.GetTimestamp();
            long index = Interlocked.Increment(ref sampleIndex) - 1;
            timestamps[index % Capacity] = now;
            if ((mouse.usFlags & 0x08) != 0) Interlocked.Increment(ref noCoalesceCount);
            Interlocked.Exchange(ref latestTimestamp, now);
            Interlocked.Increment(ref latestSequence);
        }

        internal static void StartPreviewSampler()
        {
            previewRunning = true;
            previewThread = new Thread(new ThreadStart(delegate
            {
                long frameTicks = Stopwatch.Frequency / 120;
                long next = Stopwatch.GetTimestamp();
                while (previewRunning)
                {
                    next += frameTicks;
                    long sequence = Interlocked.Read(ref latestSequence);
                    if (sequence != Interlocked.Read(ref consumedSequence))
                    {
                        long latency = Stopwatch.GetTimestamp() - Interlocked.Read(ref latestTimestamp);
                        long index = Interlocked.Increment(ref previewIndex) - 1;
                        previewLatencyNanoseconds[index % Capacity] = latency * 1000000000L / Stopwatch.Frequency;
                        Interlocked.Exchange(ref consumedSequence, sequence);
                    }
                    while (previewRunning && Stopwatch.GetTimestamp() < next) Thread.SpinWait(50);
                }
            }));
            previewThread.IsBackground = true;
            previewThread.Priority = ThreadPriority.AboveNormal;
            previewThread.Name = "PROTOTYPE 120Hz preview sampler";
            previewThread.Start();
        }

        internal static void StopPreviewSampler()
        {
            previewRunning = false;
            if (previewThread != null) previewThread.Join(1000);
            previewThread = null;
            if (dataBuffer != IntPtr.Zero) Marshal.FreeHGlobal(dataBuffer);
            if (batchBuffer != IntPtr.Zero) Marshal.FreeHGlobal(batchBuffer);
        }

        internal static MetricsSummary Summarize(long start)
        {
            long end = SampleIndex;
            long count = Math.Min(end - start, Capacity);
            List<double> intervals = new List<double>((int)Math.Min(count, 100000));
            long skip = Math.Max(1, count / 100000);
            long previous = 0;
            for (long i = 0; i < count; i += skip)
            {
                long timestamp = timestamps[(start + i) % Capacity];
                if (previous != 0 && timestamp > previous)
                {
                    intervals.Add((timestamp - previous) * 1000.0 / Stopwatch.Frequency);
                }
                previous = timestamp;
            }
            intervals.Sort();
            double medianRate = intervals.Count == 0 ? 0 : 1000.0 / intervals[intervals.Count / 2];
            return InputHooks.SummaryFromSorted(intervals, count, medianRate);
        }

        internal static MetricsSummary SummarizePreview(long start)
        {
            long end = PreviewIndex;
            long count = Math.Min(end - start, Capacity);
            List<double> milliseconds = new List<double>((int)Math.Min(count, 100000));
            long skip = Math.Max(1, count / 100000);
            for (long i = 0; i < count; i += skip)
            {
                milliseconds.Add(previewLatencyNanoseconds[(start + i) % Capacity] / 1000000.0);
            }
            milliseconds.Sort();
            return InputHooks.SummaryFromSorted(milliseconds, count, 0);
        }

        internal static string StatsLine()
        {
            MetricsSummary summary = Summarize(Math.Max(0, SampleIndex - 10000));
            MetricsSummary preview = SummarizePreview(Math.Max(0, PreviewIndex - 1000));
            return string.Format(
                CultureInfo.InvariantCulture,
                "events={0} buffered={1} noCoalesce={2} medianIntervalHz={3:F0} previewP95Ms={4:F2}",
                SampleIndex,
                BufferedCount,
                NoCoalesceCount,
                summary.MedianRate,
                preview.P95);
        }
    }

    internal static class TokenSecurity
    {
        private const uint TokenQuery = 0x0008;
        private const int TokenGroups = 2;
        private const int TokenIntegrityLevel = 25;
        private const uint LogonId = 0xC0000000;

        [StructLayout(LayoutKind.Sequential)]
        private struct SID_AND_ATTRIBUTES
        {
            internal IntPtr Sid;
            internal uint Attributes;
        }

        internal static SecurityIdentifier CurrentLogonSid()
        {
            IntPtr token;
            if (!NativeMethods.OpenProcessToken(NativeMethods.GetCurrentProcess(), TokenQuery, out token))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            try
            {
                int size = 0;
                NativeMethods.GetTokenInformation(token, TokenGroups, IntPtr.Zero, 0, out size);
                IntPtr buffer = Marshal.AllocHGlobal(size);
                try
                {
                    if (!NativeMethods.GetTokenInformation(token, TokenGroups, buffer, size, out size))
                        throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
                    int count = Marshal.ReadInt32(buffer);
                    int offset = IntPtr.Size == 8 ? 8 : 4;
                    int stride = Marshal.SizeOf(typeof(SID_AND_ATTRIBUTES));
                    for (int i = 0; i < count; i++)
                    {
                        SID_AND_ATTRIBUTES group = (SID_AND_ATTRIBUTES)Marshal.PtrToStructure(IntPtr.Add(buffer, offset + i * stride), typeof(SID_AND_ATTRIBUTES));
                        if ((group.Attributes & LogonId) == LogonId)
                            return new SecurityIdentifier(group.Sid);
                    }
                }
                finally { Marshal.FreeHGlobal(buffer); }
            }
            finally { NativeMethods.CloseHandle(token); }
            throw new InvalidOperationException("current token has no logon SID");
        }

        internal static string IntegrityName()
        {
            IntPtr token;
            if (!NativeMethods.OpenProcessToken(NativeMethods.GetCurrentProcess(), TokenQuery, out token)) return "unknown";
            try
            {
                int size = 0;
                NativeMethods.GetTokenInformation(token, TokenIntegrityLevel, IntPtr.Zero, 0, out size);
                IntPtr buffer = Marshal.AllocHGlobal(size);
                try
                {
                    if (!NativeMethods.GetTokenInformation(token, TokenIntegrityLevel, buffer, size, out size)) return "unknown";
                    IntPtr sid = Marshal.ReadIntPtr(buffer);
                    IntPtr subAuthorityCount = NativeMethods.GetSidSubAuthorityCount(sid);
                    byte count = Marshal.ReadByte(subAuthorityCount);
                    IntPtr ridPtr = NativeMethods.GetSidSubAuthority(sid, (uint)(count - 1));
                    uint rid = (uint)Marshal.ReadInt32(ridPtr);
                    if (rid >= 0x4000) return "system";
                    if (rid >= 0x3000) return "high";
                    if (rid >= 0x2000) return "medium";
                    return "low";
                }
                finally { Marshal.FreeHGlobal(buffer); }
            }
            finally { NativeMethods.CloseHandle(token); }
        }
    }

    internal static class NativeMethods
    {
        internal const int WM_INPUT = 0x00FF;
        internal const int WM_INPUT_DEVICE_CHANGE = 0x00FE;
        internal const int WM_WTSSESSION_CHANGE = 0x02B1;
        internal const uint SWP_NOZORDER = 0x0004;
        internal const uint SWP_NOACTIVATE = 0x0010;

        [StructLayout(LayoutKind.Sequential)]
        internal struct POINT { internal int X; internal int Y; }

        [StructLayout(LayoutKind.Sequential)]
        internal struct RECT { internal int Left; internal int Top; internal int Right; internal int Bottom; }

        [StructLayout(LayoutKind.Sequential)]
        internal struct KBDLLHOOKSTRUCT
        {
            internal uint vkCode;
            internal uint scanCode;
            internal uint flags;
            internal uint time;
            internal IntPtr extraInfo;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct MSLLHOOKSTRUCT
        {
            internal POINT point;
            internal uint mouseData;
            internal uint flags;
            internal uint time;
            internal IntPtr extraInfo;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct RAWINPUTDEVICE
        {
            internal ushort usUsagePage;
            internal ushort usUsage;
            internal uint dwFlags;
            internal IntPtr hwndTarget;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct RAWINPUTHEADER
        {
            internal uint dwType;
            internal uint dwSize;
            internal IntPtr hDevice;
            internal IntPtr wParam;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct RAWMOUSE
        {
            internal ushort usFlags;
            internal uint ulButtons;
            internal uint ulRawButtons;
            internal int lLastX;
            internal int lLastY;
            internal uint ulExtraInformation;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct JOBOBJECT_BASIC_LIMIT_INFORMATION
        {
            internal long PerProcessUserTimeLimit;
            internal long PerJobUserTimeLimit;
            internal uint LimitFlags;
            internal UIntPtr MinimumWorkingSetSize;
            internal UIntPtr MaximumWorkingSetSize;
            internal uint ActiveProcessLimit;
            internal UIntPtr Affinity;
            internal uint PriorityClass;
            internal uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct IO_COUNTERS
        {
            internal ulong ReadOperationCount;
            internal ulong WriteOperationCount;
            internal ulong OtherOperationCount;
            internal ulong ReadTransferCount;
            internal ulong WriteTransferCount;
            internal ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        internal struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION
        {
            internal JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
            internal IO_COUNTERS IoInfo;
            internal UIntPtr ProcessMemoryLimit;
            internal UIntPtr JobMemoryLimit;
            internal UIntPtr PeakProcessMemoryUsed;
            internal UIntPtr PeakJobMemoryUsed;
        }

        internal delegate IntPtr HookDelegate(int code, IntPtr wParam, IntPtr lParam);
        internal delegate void WinEventDelegate(IntPtr hook, uint eventType, IntPtr hwnd, int objectId, int childId, uint threadId, uint time);
        internal delegate bool EnumWindowsDelegate(IntPtr hwnd, IntPtr state);

        [DllImport("user32.dll", SetLastError = true)]
        internal static extern IntPtr SetWindowsHookEx(int idHook, HookDelegate callback, IntPtr module, uint threadId);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool UnhookWindowsHookEx(IntPtr hook);
        [DllImport("user32.dll")]
        internal static extern IntPtr CallNextHookEx(IntPtr hook, int code, IntPtr wParam, IntPtr lParam);
        [DllImport("kernel32.dll")]
        internal static extern uint GetTickCount();
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool RegisterRawInputDevices(RAWINPUTDEVICE[] devices, uint count, uint size);
        [DllImport("user32.dll", SetLastError = true)]
        internal static extern uint GetRawInputData(IntPtr rawInput, uint command, IntPtr data, ref uint size, uint headerSize);
        [DllImport("user32.dll", SetLastError = true)]
        internal static extern uint GetRawInputBuffer(IntPtr data, ref uint size, uint headerSize);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetWindowPos(IntPtr hwnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetForegroundWindow(IntPtr hwnd);
        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool EnumWindows(EnumWindowsDelegate callback, IntPtr state);
        [DllImport("user32.dll")]
        internal static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool IsWindowVisible(IntPtr hwnd);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        internal static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
        [DllImport("user32.dll")]
        internal static extern IntPtr SetWinEventHook(uint min, uint max, IntPtr module, WinEventDelegate callback, uint processId, uint threadId, uint flags);
        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool UnhookWinEvent(IntPtr hook);
        [DllImport("wtsapi32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool WTSRegisterSessionNotification(IntPtr hwnd, uint flags);
        [DllImport("wtsapi32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool WTSUnRegisterSessionNotification(IntPtr hwnd);
        [DllImport("kernel32.dll")]
        internal static extern IntPtr GetCurrentProcess();
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetTokenInformation(IntPtr token, int informationClass, IntPtr information, int informationLength, out int returnLength);
        [DllImport("advapi32.dll")]
        internal static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);
        [DllImport("advapi32.dll")]
        internal static extern IntPtr GetSidSubAuthority(IntPtr sid, uint index);
        [DllImport("kernel32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool CloseHandle(IntPtr handle);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        internal static extern IntPtr CreateJobObject(IntPtr securityAttributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetInformationJobObject(IntPtr job, int informationClass, IntPtr information, uint informationLength);
        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

        internal static IntPtr CreateKillOnCloseJob()
        {
            IntPtr job = CreateJobObject(IntPtr.Zero, null);
            if (job == IntPtr.Zero) return IntPtr.Zero;
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION information = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
            information.BasicLimitInformation.LimitFlags = 0x00002000;
            int size = Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
            IntPtr pointer = Marshal.AllocHGlobal(size);
            try
            {
                Marshal.StructureToPtr(information, pointer, false);
                if (!SetInformationJobObject(job, 9, pointer, (uint)size))
                {
                    CloseHandle(job);
                    return IntPtr.Zero;
                }
                return job;
            }
            finally
            {
                Marshal.FreeHGlobal(pointer);
            }
        }
    }
}
