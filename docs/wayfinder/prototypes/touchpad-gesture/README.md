# Disposable touchpad gesture prototype

This branch answers Wayfinder's touchpad question without changing the active installation.

`native-probe` inventories HID and pointer devices, checks for the Precision Touchpad top-level HID collection, and calls `TouchpadGesturesController.IsSupported`. It does not register gesture handlers, suppress shell gestures, change Windows settings, or inject input.

Run it with:

```powershell
cargo run --manifest-path native-probe/Cargo.toml
```

The gesture-session logic prototype will remain separate from this device probe. Physical reliability and cadence claims require a present Precision Touchpad and real contact input.

Open `gesture-session-prototype.html` directly for the pure state model. See `RESULTS.md` for the selected route, typed call stacks, current machine evidence, and the remaining physical completion contract.
