# Split shell hosts by Windows role

Use one shared Rust shell implementation but run AppBar, interactive, notification, and OSD roles in separate GUI-subsystem processes. The roles share typed snapshots, intents, theme tokens, accessibility semantics, and GPUI projection code, but never mutable state or a crash boundary; this keeps the palette, overview, and quick controls cohesive while preventing their failure from taking down an AppBar reservation or a proved notification/OSD route.

The manager remains the only authoritative state owner. GPUI owns pixels and accessible presentation, each role host owns its HWND-affine Windows resources, and a manager-issued generation-fenced lease is required before a process can act as a role. A single shell process was rejected because it couples unrelated Windows effects and recovery, while a process per surface was rejected because it duplicates focus, renderer, and accessibility sessions inside the mutually exclusive interactive feature set.
