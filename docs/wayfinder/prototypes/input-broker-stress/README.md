# Input broker stress prototype

This is throwaway code for the Wayfinder ticket [Prototype the input broker under privilege and device stress](https://github.com/themixednuts/komorebi/issues/25). It is not part of komorebi and must not ship.

Run `./run.ps1` from PowerShell. The observer stays at medium integrity. Starting the broker opens one ordinary UAC consent prompt. The elevated broker exposes only ping, move, focus, hook-mode, cancellation, statistics, crash, and stop commands over a named pipe whose ACL contains only the current logon SID.

The prototype never synthesizes input. F12 and either mouse side button are the physical reference inputs. It never runs on, captures, draws over, or automates the secure desktop.

The installed Logitech G305 is limited by its manufacturer to a 1 ms, 1000 Hz report rate. This machine can measure the 1000 Hz case. During each timed sample, move the mouse continuously for the full countdown. The 4000 and 8000 Hz cases require different physical hardware and remain untested.

The UI writes nothing until **Save report** is pressed. Saved reports go under `results/` and are intended to be committed only to this throwaway branch as primary evidence.
