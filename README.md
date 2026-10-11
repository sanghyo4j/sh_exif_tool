# SH148 Photo Management Tool

## UI build selection

`cargo build --release` builds the UI listed in the `default` feature in `Cargo.toml`.

```toml
default = ["gui-win32"] # Default product build
# default = ["gui-slint"] # Legacy Slint build
```

Keep exactly one of `gui-win32` or `gui-slint` in the default feature list. The build command stays the same.

Release executables use a fixed two-digit build number (`build01` through `build99`).
The same version and build number must appear in the window title, for example:
`SH148 Photo Management Tool v0.1.0 (Build 67)`.
