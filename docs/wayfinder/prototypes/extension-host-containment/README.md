# Disposable extension-host containment prototype

This throwaway prototype answers [Prototype restricted extension-host containment and brokered I/O](https://github.com/themixednuts/komorebi/issues/39). It measures whether a dedicated Rust process can host one LuaJIT extension behind Windows LPAC, Job Object, process mitigation, and authenticated named-pipe boundaries on the target machine.

The native harness creates unique test AppContainer profiles and removes them after each run. It stages only disposable files under those profiles. It does not alter the active manager installation.

```powershell
.\native\run.ps1
```

The runner statically links the MSVC CRT into both children. LPAC deliberately removes ambient access to the system VC runtime, so a normal desktop Rust build is not a valid containment test.

The result is written to `results/latest.json`. The report records the selected dedicated-process design and every measured pass, denial, limitation, and unresolved proof obligation directly. There is no interactive decision layer.

The launch and probe stack keeps Windows paths and environment values in native `OsStr`/`OsString` form. The suite includes a real package filename with an unpaired UTF-16 surrogate and records lossless UTF-16 evidence alongside optional UTF-8 display text.

The harness uses one narrowly scoped compatibility capability, `lpacAppExperience`. It does not grant registry, COM, clipboard, or network capabilities. The adversarial probes verify that those surfaces remain denied.

This branch is evidence only. None of the code is production extension infrastructure.
