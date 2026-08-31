# komorebi source fork

This directory vendors `fff-search` 0.10.7-nightly.f3647b7 from the published
crate whose upstream repository is <https://github.com/dmtrKovalenko/fff>.
The upstream package declares the MIT license; its published source metadata is
preserved here.

The fork exists because upstream stores indexed paths as lossy UTF-8 strings.
On Windows that can replace unpaired UTF-16 code units and later target a
different path. Komorebi requires search presentation to remain separate from
the exact `PathBuf` used for file I/O and shell activation.

The local delta changes `FilePickerOptions::base_path` from `String` to
`PathBuf` and gives `FileItem` an optional exact Windows path. That allocation
is populated only when `Path::to_str()` fails, so ordinary Unicode paths retain
the upstream memory shape. `absolute_path` and internal filesystem consumers
prefer the exact operand when present. Windows canonicalization may correctly
retain a `\\?\` prefix for paths containing unpaired surrogates; the regression
test treats that prefix as syntax rather than part of the filename identity.

Local changes must remain narrowly marked with `komorebi:` comments and covered
by `tests/windows_wtf16_paths.rs`. Syncing a newer upstream version requires
that regression to pass before the dependency can be updated.
