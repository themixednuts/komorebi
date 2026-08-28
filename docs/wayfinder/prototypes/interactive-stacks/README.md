# Disposable interactive stacks prototype

This throwaway HTML prototype compares three interaction structures for window stacking, splitting, exact stackbar insertion, locks, cancellation, stale-target recovery, keyboard parity, and cross-monitor placement.

Open `interactive-stacks-prototype.html` directly. Use the bottom switcher or `?variant=direct`, `?variant=rail`, and `?variant=compass`.

Run `node interaction-model.test.cjs` to exercise the shared placement model without a browser. The checks cover stack, exact tab reorder, split side, locked rejection, cancellation, cross-monitor placement, stale-target recovery, and keyboard parity.

The simulation does not call native APIs, persist state, or represent production renderer architecture. It exists only to settle the behavior contract for Wayfinder issue #12.
