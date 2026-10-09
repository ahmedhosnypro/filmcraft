# FilmCraft desktop host

The native host connects the shared egui UI to audio devices, file dialogs, the control
server and platform media APIs. See [contributing](../../docs/contributing.md) for running
the app and [the control protocol](../../docs/control-protocol.md) for automation.

## File dialogs

All filtered file dialogs use `file_filters::extensions`. On Linux and the BSDs, rfd's
XDG portal and Zenity backends use case-sensitive globs. The helper adds bracket patterns
so camera files such as `shot.MP4` and `shot.Mp4` appear alongside `shot.mp4`. This also
applies to relinking, opening projects and presets, and save dialogs. Literal extensions
remain first for the default save suffix; macOS and Windows receive ordinary extensions.

`cargo test --release -p filmcraft --bin filmcraft` checks matching and rejection of
filenames, including mixed case and misleading suffixes, and preserves default save
extensions. The filters follow the
[XDG FileChooser contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html).
