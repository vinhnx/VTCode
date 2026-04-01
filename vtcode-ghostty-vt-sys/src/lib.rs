use anyhow::{Context, Result, anyhow};
use libloading::Library;
use std::ffi::c_void;
use std::os::raw::{c_int, c_uint};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub struct GhosttyRenderRequest {
    pub cols: u16,
    pub rows: u16,
    pub scrollback_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyRenderOutput {
    pub screen_contents: String,
    pub scrollback: String,
}

pub fn render_terminal_snapshot(
    request: GhosttyRenderRequest,
    vt_stream: &[u8],
) -> Result<GhosttyRenderOutput> {
    if vt_stream.is_empty() {
        return Ok(GhosttyRenderOutput {
            screen_contents: String::new(),
            scrollback: String::new(),
        });
    }

    platform::render_terminal_snapshot(request, vt_stream)
}

fn unavailable_error() -> anyhow::Error {
    anyhow!("Ghostty VT library is unavailable; falling back to legacy_vt100")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use super::{
        Context, GhosttyRenderOutput, GhosttyRenderRequest, Library, OnceLock, Path, PathBuf,
        Result, anyhow, c_int, c_uint, c_void, unavailable_error,
    };
    use std::cmp;
    use std::mem;

    const GHOSTTY_SUCCESS: c_int = 0;
    const GHOSTTY_OUT_OF_SPACE: c_int = -3;

    const GHOSTTY_CELL_DATA_CODEPOINT: c_uint = 1;
    const GHOSTTY_CELL_DATA_WIDE: c_uint = 3;
    const GHOSTTY_CELL_DATA_HAS_TEXT: c_uint = 4;
    const GHOSTTY_CELL_WIDE_SPACER_TAIL: c_uint = 2;
    const GHOSTTY_CELL_WIDE_SPACER_HEAD: c_uint = 3;
    const GHOSTTY_ROW_DATA_WRAP: c_uint = 1;
    const GHOSTTY_POINT_TAG_ACTIVE: c_uint = 0;
    const GHOSTTY_POINT_TAG_SCREEN: c_uint = 2;
    const GHOSTTY_TERMINAL_DATA_SCROLLBAR: c_uint = 9;

    type GhosttyTerminal = *mut c_void;
    type GhosttyCell = u64;
    type GhosttyRow = u64;
    type GhosttyResult = c_int;
    type GhosttyTerminalNew = unsafe extern "C" fn(
        allocator: *const GhosttyAllocator,
        terminal: *mut GhosttyTerminal,
        options: GhosttyTerminalOptions,
    ) -> GhosttyResult;
    type GhosttyTerminalFree = unsafe extern "C" fn(terminal: GhosttyTerminal);
    type GhosttyTerminalGet = unsafe extern "C" fn(
        terminal: GhosttyTerminal,
        data: c_uint,
        out: *mut c_void,
    ) -> GhosttyResult;
    type GhosttyTerminalGridRef = unsafe extern "C" fn(
        terminal: GhosttyTerminal,
        point: GhosttyPoint,
        out_ref: *mut GhosttyGridRef,
    ) -> GhosttyResult;
    type GhosttyTerminalVtWrite =
        unsafe extern "C" fn(terminal: GhosttyTerminal, data: *const u8, len: usize);
    type GhosttyGridRefCell = unsafe extern "C" fn(
        ref_: *const GhosttyGridRef,
        out_cell: *mut GhosttyCell,
    ) -> GhosttyResult;
    type GhosttyGridRefRow = unsafe extern "C" fn(
        ref_: *const GhosttyGridRef,
        out_row: *mut GhosttyRow,
    ) -> GhosttyResult;
    type GhosttyGridRefGraphemes = unsafe extern "C" fn(
        ref_: *const GhosttyGridRef,
        buf: *mut u32,
        buf_len: usize,
        out_len: *mut usize,
    ) -> GhosttyResult;
    type GhosttyCellGet =
        unsafe extern "C" fn(cell: GhosttyCell, data: c_uint, out: *mut c_void) -> GhosttyResult;
    type GhosttyRowGet =
        unsafe extern "C" fn(row: GhosttyRow, data: c_uint, out: *mut c_void) -> GhosttyResult;

    #[repr(C)]
    struct GhosttyAllocator {
        _unused: [u8; 0],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct GhosttyTerminalOptions {
        cols: u16,
        rows: u16,
        max_scrollback: usize,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct GhosttyGridRef {
        size: usize,
        node: *mut c_void,
        x: u16,
        y: u16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct GhosttyPointCoordinate {
        x: u16,
        y: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    union GhosttyPointValue {
        coordinate: GhosttyPointCoordinate,
        padding: [u64; 2],
    }

    impl Default for GhosttyPointValue {
        fn default() -> Self {
            Self { padding: [0; 2] }
        }
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct GhosttyPoint {
        tag: c_uint,
        value: GhosttyPointValue,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct GhosttyTerminalScrollbar {
        total: u64,
        offset: u64,
        len: u64,
    }

    #[derive(Debug)]
    struct GhosttyApi {
        _library: Library,
        terminal_new: GhosttyTerminalNew,
        terminal_free: GhosttyTerminalFree,
        terminal_get: GhosttyTerminalGet,
        terminal_grid_ref: GhosttyTerminalGridRef,
        terminal_vt_write: GhosttyTerminalVtWrite,
        grid_ref_cell: GhosttyGridRefCell,
        grid_ref_row: GhosttyGridRefRow,
        grid_ref_graphemes: GhosttyGridRefGraphemes,
        cell_get: GhosttyCellGet,
        row_get: GhosttyRowGet,
    }

    struct TerminalHandle<'a> {
        api: &'a GhosttyApi,
        raw: GhosttyTerminal,
    }

    impl Drop for TerminalHandle<'_> {
        fn drop(&mut self) {
            if self.raw.is_null() {
                return;
            }
            // SAFETY: `raw` is owned by this handle and was returned by `ghostty_terminal_new`.
            unsafe { (self.api.terminal_free)(self.raw) };
        }
    }

    pub(super) fn render_terminal_snapshot(
        request: GhosttyRenderRequest,
        vt_stream: &[u8],
    ) -> Result<GhosttyRenderOutput> {
        let api = GhosttyApi::load()?;
        render_snapshot_with_api(api, request, vt_stream)
    }

    impl GhosttyApi {
        fn load() -> Result<&'static Self> {
            static API: OnceLock<std::result::Result<GhosttyApi, String>> = OnceLock::new();

            match API.get_or_init(|| {
                Self::load_from_dirs(runtime_library_dirs()).map_err(|error| error.to_string())
            }) {
                Ok(api) => Ok(api),
                Err(error) => Err(anyhow!(error.clone())),
            }
        }

        fn load_from_dirs(dirs: Vec<PathBuf>) -> Result<Self> {
            let candidates = candidate_library_paths_from_dirs(dirs);
            if candidates.is_empty() {
                return Err(unavailable_error());
            }

            let mut errors = Vec::new();
            for candidate in candidates {
                match Self::load_from_path(&candidate) {
                    Ok(api) => return Ok(api),
                    Err(error) => errors.push(format!("{}: {error}", candidate.display())),
                }
            }

            Err(anyhow!("{} ({})", unavailable_error(), errors.join("; ")))
        }

        fn load_from_path(path: &Path) -> Result<Self> {
            // SAFETY: Loading the packaged Ghostty runtime library is inherently unsafe. The path
            // comes from VT Code-controlled package locations and the symbols are validated below.
            let library = unsafe { Library::new(path) }
                .with_context(|| format!("failed to load {}", path.display()))?;

            Ok(Self {
                terminal_new: load_symbol(&library, b"ghostty_terminal_new\0")?,
                terminal_free: load_symbol(&library, b"ghostty_terminal_free\0")?,
                terminal_get: load_symbol(&library, b"ghostty_terminal_get\0")?,
                terminal_grid_ref: load_symbol(&library, b"ghostty_terminal_grid_ref\0")?,
                terminal_vt_write: load_symbol(&library, b"ghostty_terminal_vt_write\0")?,
                grid_ref_cell: load_symbol(&library, b"ghostty_grid_ref_cell\0")?,
                grid_ref_row: load_symbol(&library, b"ghostty_grid_ref_row\0")?,
                grid_ref_graphemes: load_symbol(&library, b"ghostty_grid_ref_graphemes\0")?,
                cell_get: load_symbol(&library, b"ghostty_cell_get\0")?,
                row_get: load_symbol(&library, b"ghostty_row_get\0")?,
                _library: library,
            })
        }
    }

    impl TerminalHandle<'_> {
        fn new(api: &GhosttyApi, request: GhosttyRenderRequest) -> Result<TerminalHandle<'_>> {
            let mut raw = std::ptr::null_mut();
            let options = GhosttyTerminalOptions {
                cols: request.cols,
                rows: request.rows,
                max_scrollback: request.scrollback_lines,
            };

            // SAFETY: `raw` points to writable storage and `options` matches the upstream layout.
            ensure_success(
                unsafe { (api.terminal_new)(std::ptr::null(), &mut raw, options) },
                "failed to create Ghostty terminal",
            )?;
            if raw.is_null() {
                return Err(anyhow!(
                    "failed to create Ghostty terminal: Ghostty returned null"
                ));
            }

            Ok(TerminalHandle { api, raw })
        }
    }

    fn render_region(
        api: &GhosttyApi,
        terminal: GhosttyTerminal,
        tag: c_uint,
        row_count: u32,
        cols: u16,
    ) -> Result<String> {
        let row_count_hint = usize::try_from(row_count).unwrap_or(usize::MAX);
        let col_count_hint = usize::from(cols);
        let capacity = row_count_hint.saturating_mul(col_count_hint.saturating_add(1));
        let mut output = String::with_capacity(capacity);

        for row in 0..row_count {
            let row_start = output.len();

            for col in 0..cols {
                let point = grid_point(tag, col, row);
                let mut grid_ref = sized::<GhosttyGridRef>();

                // SAFETY: `terminal` is valid, `point` is initialized, and `grid_ref` points to
                // writable storage for the returned opaque reference.
                let ref_result = unsafe { (api.terminal_grid_ref)(terminal, point, &mut grid_ref) };
                if ref_result != GHOSTTY_SUCCESS {
                    output.push(' ');
                    continue;
                }

                let mut cell = 0;
                // SAFETY: `grid_ref` is initialized and `cell` points to writable storage.
                let cell_result = unsafe { (api.grid_ref_cell)(&grid_ref, &mut cell) };
                if cell_result != GHOSTTY_SUCCESS {
                    output.push(' ');
                    continue;
                }

                append_cell_text(api, &mut output, &grid_ref, cell)?;
            }

            let wrap = row_wraps(api, terminal, tag, row)?;
            trim_trailing_spaces(&mut output, row_start);
            if !wrap && row + 1 < row_count {
                output.push('\n');
            }
        }

        Ok(output)
    }

    fn append_cell_text(
        api: &GhosttyApi,
        output: &mut String,
        grid_ref: &GhosttyGridRef,
        cell: GhosttyCell,
    ) -> Result<()> {
        let mut wide = 0u32;
        // SAFETY: `wide` points to writable storage for the requested cell width field.
        ensure_success(
            unsafe { (api.cell_get)(cell, GHOSTTY_CELL_DATA_WIDE, (&mut wide as *mut u32).cast()) },
            "failed to read Ghostty cell width",
        )?;

        if wide == GHOSTTY_CELL_WIDE_SPACER_TAIL || wide == GHOSTTY_CELL_WIDE_SPACER_HEAD {
            return Ok(());
        }

        let mut has_text = false;
        // SAFETY: `has_text` points to writable storage for the requested boolean field.
        ensure_success(
            unsafe {
                (api.cell_get)(
                    cell,
                    GHOSTTY_CELL_DATA_HAS_TEXT,
                    (&mut has_text as *mut bool).cast(),
                )
            },
            "failed to read Ghostty cell text flag",
        )?;

        if !has_text {
            output.push(' ');
            return Ok(());
        }

        let mut grapheme_len = 0usize;
        // SAFETY: Passing a null buffer is the documented way to query grapheme length.
        let grapheme_result = unsafe {
            (api.grid_ref_graphemes)(grid_ref, std::ptr::null_mut(), 0, &mut grapheme_len)
        };
        if grapheme_result == GHOSTTY_OUT_OF_SPACE && grapheme_len > 0 {
            let mut codepoints = vec![0u32; grapheme_len];
            // SAFETY: `codepoints` provides writable storage for the reported grapheme length.
            ensure_success(
                unsafe {
                    (api.grid_ref_graphemes)(
                        grid_ref,
                        codepoints.as_mut_ptr(),
                        codepoints.len(),
                        &mut grapheme_len,
                    )
                },
                "failed to read Ghostty grapheme cluster",
            )?;

            for codepoint in codepoints.into_iter().take(grapheme_len) {
                push_codepoint(output, codepoint);
            }
            return Ok(());
        }

        let mut codepoint = 0u32;
        // SAFETY: `codepoint` points to writable storage for the requested codepoint field.
        ensure_success(
            unsafe {
                (api.cell_get)(
                    cell,
                    GHOSTTY_CELL_DATA_CODEPOINT,
                    (&mut codepoint as *mut u32).cast(),
                )
            },
            "failed to read Ghostty codepoint",
        )?;

        push_codepoint(output, codepoint);
        Ok(())
    }

    fn row_wraps(
        api: &GhosttyApi,
        terminal: GhosttyTerminal,
        tag: c_uint,
        row: u32,
    ) -> Result<bool> {
        let mut grid_ref = sized::<GhosttyGridRef>();
        // SAFETY: `grid_ref` points to writable storage for the requested row reference.
        let ref_result =
            unsafe { (api.terminal_grid_ref)(terminal, grid_point(tag, 0, row), &mut grid_ref) };
        if ref_result != GHOSTTY_SUCCESS {
            return Ok(false);
        }

        let mut grid_row = 0;
        // SAFETY: `grid_row` points to writable storage for the row handle.
        let row_result = unsafe { (api.grid_ref_row)(&grid_ref, &mut grid_row) };
        if row_result != GHOSTTY_SUCCESS {
            return Ok(false);
        }

        let mut wrap = false;
        // SAFETY: `wrap` points to writable storage for the requested row field.
        let wrap_result = unsafe {
            (api.row_get)(
                grid_row,
                GHOSTTY_ROW_DATA_WRAP,
                (&mut wrap as *mut bool).cast(),
            )
        };
        if wrap_result != GHOSTTY_SUCCESS {
            return Ok(false);
        }

        Ok(wrap)
    }

    fn runtime_library_dirs() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Ok(current_exe) = std::env::current_exe()
            && let Some(exe_dir) = current_exe.parent()
        {
            push_unique(&mut roots, exe_dir.join("ghostty-vt"));
            push_unique(&mut roots, exe_dir.to_path_buf());
        }
        roots
    }

    fn render_snapshot_with_api(
        api: &GhosttyApi,
        request: GhosttyRenderRequest,
        vt_stream: &[u8],
    ) -> Result<GhosttyRenderOutput> {
        let terminal = TerminalHandle::new(api, request)?;

        // SAFETY: `terminal.raw` is valid for the duration of the call and the slice pointer/len
        // pair remains valid across the FFI call.
        unsafe { (api.terminal_vt_write)(terminal.raw, vt_stream.as_ptr(), vt_stream.len()) };

        let screen_contents = render_region(
            api,
            terminal.raw,
            GHOSTTY_POINT_TAG_ACTIVE,
            u32::from(request.rows),
            request.cols,
        )?;

        let total_rows = query_total_rows(api, terminal.raw, request.rows)?;
        let scrollback = render_region(
            api,
            terminal.raw,
            GHOSTTY_POINT_TAG_SCREEN,
            total_rows,
            request.cols,
        )?;

        Ok(GhosttyRenderOutput {
            screen_contents,
            scrollback,
        })
    }

    fn query_total_rows(api: &GhosttyApi, terminal: GhosttyTerminal, rows: u16) -> Result<u32> {
        let mut scrollbar = GhosttyTerminalScrollbar::default();
        // SAFETY: `scrollbar` points to writable storage for the requested result type.
        ensure_success(
            unsafe {
                (api.terminal_get)(
                    terminal,
                    GHOSTTY_TERMINAL_DATA_SCROLLBAR,
                    (&mut scrollbar as *mut GhosttyTerminalScrollbar).cast(),
                )
            },
            "failed to query Ghostty scrollbar state",
        )?;

        let total_rows = cmp::max(scrollbar.total, u64::from(rows));
        u32::try_from(total_rows)
            .map_err(|_| anyhow!("Ghostty screen too large to render: {total_rows}"))
    }

    fn candidate_library_paths_from_dirs(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };

            let mut preferred = Vec::new();
            let mut versioned = Vec::new();

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if !is_runtime_library_name(name) {
                    continue;
                }

                if is_preferred_runtime_library_name(name) {
                    preferred.push(path);
                } else {
                    versioned.push(path);
                }
            }

            preferred.sort();
            versioned.sort();
            paths.extend(preferred);
            paths.extend(versioned);
        }

        paths
    }

    fn is_runtime_library_name(name: &str) -> bool {
        if cfg!(target_os = "macos") {
            name.starts_with("libghostty-vt") && name.ends_with(".dylib")
        } else {
            name.starts_with("libghostty-vt") && name.contains(".so")
        }
    }

    fn is_preferred_runtime_library_name(name: &str) -> bool {
        if cfg!(target_os = "macos") {
            name == "libghostty-vt.dylib"
        } else {
            name == "libghostty-vt.so"
        }
    }

    fn grid_point(tag: c_uint, x: u16, y: u32) -> GhosttyPoint {
        GhosttyPoint {
            tag,
            value: GhosttyPointValue {
                coordinate: GhosttyPointCoordinate { x, y },
            },
        }
    }

    fn ensure_success(result: GhosttyResult, context: &str) -> Result<()> {
        if result == GHOSTTY_SUCCESS {
            Ok(())
        } else {
            Err(anyhow!("{context}: Ghostty returned {result}"))
        }
    }

    fn push_codepoint(output: &mut String, codepoint: u32) {
        if let Some(ch) = char::from_u32(codepoint) {
            output.push(ch);
        } else {
            output.push(char::REPLACEMENT_CHARACTER);
        }
    }

    fn trim_trailing_spaces(output: &mut String, floor: usize) {
        while output.len() > floor && output.as_bytes().last().copied() == Some(b' ') {
            let _ = output.pop();
        }
    }

    fn push_unique(values: &mut Vec<PathBuf>, value: PathBuf) {
        if !values.iter().any(|existing| existing == &value) {
            values.push(value);
        }
    }

    fn sized<T: Default>() -> T {
        let mut value = T::default();
        // SAFETY: All callers use FFI structs whose first field is the `size` field expected by
        // Ghostty. This mirrors the upstream `sized!` helper from `libghostty-rs`.
        unsafe {
            let size_ptr = (&mut value as *mut T).cast::<usize>();
            *size_ptr = mem::size_of::<T>();
        }
        value
    }

    fn load_symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
        // SAFETY: The symbol names and types match the upstream `libghostty-rs` bindings.
        let symbol = unsafe { library.get::<T>(name) }
            .with_context(|| format!("missing symbol {}", String::from_utf8_lossy(name)))?;
        Ok(*symbol)
    }

    #[cfg(test)]
    fn test_asset_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(asset_dir) = option_env!("VTCODE_GHOSTTY_VT_TEST_ASSET_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            push_unique(&mut dirs, asset_dir.join("lib"));
            push_unique(&mut dirs, asset_dir);
        }
        dirs
    }

    #[cfg(test)]
    fn real_ghostty_available() -> bool {
        !test_asset_dirs().is_empty() && GhosttyApi::load_from_dirs(test_asset_dirs()).is_ok()
    }

    #[cfg(test)]
    fn render_with_test_assets(
        request: GhosttyRenderRequest,
        vt_stream: &[u8],
    ) -> Result<GhosttyRenderOutput> {
        let api = GhosttyApi::load_from_dirs(test_asset_dirs())?;
        render_snapshot_with_api(&api, request, vt_stream)
    }

    #[cfg(test)]
    mod tests {
        use super::{GhosttyApi, candidate_library_paths_from_dirs, real_ghostty_available};
        use crate::{GhosttyRenderOutput, GhosttyRenderRequest};

        #[test]
        fn empty_dirs_report_unavailable() {
            let error = GhosttyApi::load_from_dirs(Vec::new()).expect_err("missing dirs must fail");
            assert!(error.to_string().contains("legacy_vt100"));
        }

        #[test]
        fn candidate_library_paths_prioritize_unversioned_names() {
            let temp = tempfile::tempdir().expect("tempdir");
            let root = temp.path();
            let preferred = if cfg!(target_os = "macos") {
                root.join("libghostty-vt.dylib")
            } else {
                root.join("libghostty-vt.so")
            };
            let versioned = if cfg!(target_os = "macos") {
                root.join("libghostty-vt.0.1.0.dylib")
            } else {
                root.join("libghostty-vt.so.0.1.0")
            };
            std::fs::write(&preferred, b"").expect("preferred");
            std::fs::write(&versioned, b"").expect("versioned");

            let candidates = candidate_library_paths_from_dirs(vec![root.to_path_buf()]);
            assert_eq!(candidates.first(), Some(&preferred));
            assert_eq!(candidates.get(1), Some(&versioned));
        }

        #[test]
        fn renders_plain_text_when_test_assets_are_available() {
            if !real_ghostty_available() {
                return;
            }

            let output = super::render_with_test_assets(
                GhosttyRenderRequest {
                    cols: 5,
                    rows: 1,
                    scrollback_lines: 16,
                },
                b"hello",
            )
            .expect("plain text should render");

            assert_eq!(
                output,
                GhosttyRenderOutput {
                    screen_contents: "hello".to_string(),
                    scrollback: "hello".to_string(),
                }
            );
        }

        #[test]
        fn wrapped_rows_do_not_insert_newlines_when_test_assets_are_available() {
            if !real_ghostty_available() {
                return;
            }

            let output = super::render_with_test_assets(
                GhosttyRenderRequest {
                    cols: 5,
                    rows: 2,
                    scrollback_lines: 16,
                },
                b"helloworld",
            )
            .expect("wrapped text should render");

            assert_eq!(output.screen_contents, "helloworld");
        }

        #[test]
        fn trims_trailing_spaces_when_test_assets_are_available() {
            if !real_ghostty_available() {
                return;
            }

            let output = super::render_with_test_assets(
                GhosttyRenderRequest {
                    cols: 6,
                    rows: 1,
                    scrollback_lines: 16,
                },
                b"hi   ",
            )
            .expect("trailing spaces should trim");

            assert_eq!(output.screen_contents, "hi");
        }

        #[test]
        fn wide_cells_skip_spacers_when_test_assets_are_available() {
            if !real_ghostty_available() {
                return;
            }

            let output = super::render_with_test_assets(
                GhosttyRenderRequest {
                    cols: 4,
                    rows: 1,
                    scrollback_lines: 16,
                },
                "你a".as_bytes(),
            )
            .expect("wide glyphs should render");

            assert_eq!(output.screen_contents, "你a");
        }

        #[test]
        fn scrollback_renders_full_screen_when_test_assets_are_available() {
            if !real_ghostty_available() {
                return;
            }

            let output = super::render_with_test_assets(
                GhosttyRenderRequest {
                    cols: 5,
                    rows: 1,
                    scrollback_lines: 16,
                },
                b"one\r\ntwo",
            )
            .expect("scrollback should render");

            assert_eq!(output.screen_contents, "two");
            assert_eq!(output.scrollback, "one\ntwo");
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use super::{GhosttyRenderOutput, GhosttyRenderRequest, Result, unavailable_error};

    pub(super) fn render_terminal_snapshot(
        _request: GhosttyRenderRequest,
        _vt_stream: &[u8],
    ) -> Result<GhosttyRenderOutput> {
        Err(unavailable_error())
    }
}

#[cfg(test)]
mod tests {
    use super::{GhosttyRenderOutput, GhosttyRenderRequest, render_terminal_snapshot};

    #[test]
    fn empty_vt_stream_returns_empty_snapshot() {
        let output = render_terminal_snapshot(
            GhosttyRenderRequest {
                cols: 80,
                rows: 24,
                scrollback_lines: 1000,
            },
            &[],
        )
        .expect("empty VT stream should not require Ghostty");

        assert_eq!(
            output,
            GhosttyRenderOutput {
                screen_contents: String::new(),
                scrollback: String::new(),
            }
        );
    }
}
