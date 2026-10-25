# GPUI macOS cursor refresh

Vendored `gpui-pre-macos` 0.3.9 (Apache-2.0), based on Zed commit
`93d745668805029476e6d7fbb19652116fed51de`. Original copyright notices and
`LICENSE-APACHE` are retained. Changes are in `src/window.rs`.

GPUI resolves hover cursors per element, while this backend registers one
AppKit cursor rectangle over its content view. Invalidating that rectangle
alone can leave the visible cursor unchanged until a later tracking event.

The patch immediately sets the resolved cursor when the pointer is inside the
key window's content view. It also reapplies an unchanged style, covering
AppKit menu dismissal. Cursor mapping is shared with `resetCursorRects`.
Window state is unlocked before invalidation, because AppKit can synchronously
invoke `resetCursorRects`, which takes the same lock. Background windows and
native title-bar controls retain AppKit's cursor behavior.

Reference: [AppKit cursor rectangles](https://developer.apple.com/documentation/appkit/nswindow/invalidatecursorrects(for:)).
This is a local backend fix, not a change to GPUI's hit testing or focus policy.
