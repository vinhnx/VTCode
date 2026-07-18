// Slice 4 wires this helper into application shortcut handling.
#![allow(dead_code)]

use std::{
    env, fs,
    io::{Cursor, Read},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use thiserror::Error;

use super::super::types::ContentPart;

const PNG_MEDIA_TYPE: &str = "image/png";
const WSL_FALLBACK_TIMEOUT: Duration = Duration::from_secs(3);
const POWERSHELL_CLIPBOARD_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$action = {
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $image = [System.Windows.Forms.Clipboard]::GetImage()
    if ($null -eq $image) { [Environment]::Exit(2) }
    $stream = New-Object System.IO.MemoryStream
    $image.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    [Console]::Out.Write([Convert]::ToBase64String($stream.ToArray()))
}
if ([System.Threading.Thread]::CurrentThread.GetApartmentState() -eq [System.Threading.ApartmentState]::STA) {
    & $action
    exit 0
}
$thread = [System.Threading.Thread]::new([System.Threading.ThreadStart]$action)
$thread.SetApartmentState([System.Threading.ApartmentState]::STA)
$thread.Start()
$thread.Join()
exit 0
"#;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum ClipboardImageError {
    #[error("no image in clipboard")]
    NoImage,
    #[error("clipboard image paste unavailable")]
    ClipboardUnavailable,
    #[error("selected model does not support image input")]
    UnsupportedModel,
    #[error("WSL PowerShell clipboard fallback failed")]
    WslFallbackFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClipboardRgbaImage {
    pub width: usize,
    pub height: usize,
    pub bytes: Vec<u8>,
}

trait ClipboardImageSource {
    fn file_paths(&mut self) -> Result<Vec<PathBuf>, ClipboardImageError>;

    fn rgba_image(&mut self) -> Result<ClipboardRgbaImage, ClipboardImageError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandRunOutput {
    status_code: Option<i32>,
    stdout: Vec<u8>,
    timed_out: bool,
}

trait ClipboardCommandRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> std::io::Result<CommandRunOutput>;
}

pub(crate) fn read_clipboard_image() -> Result<ContentPart, ClipboardImageError> {
    let wsl = is_wsl();

    #[cfg(not(target_os = "android"))]
    {
        let mut source = match ArboardClipboardSource::new() {
            Ok(source) => source,
            Err(_) if wsl => return read_wsl_fallback_image(&SystemCommandRunner),
            Err(error) => return Err(error),
        };
        read_clipboard_image_with(&mut source, &SystemCommandRunner, wsl)
    }

    #[cfg(target_os = "android")]
    {
        if wsl {
            read_wsl_fallback_image(&SystemCommandRunner)
        } else {
            Err(ClipboardImageError::ClipboardUnavailable)
        }
    }
}

fn read_clipboard_image_with(
    source: &mut impl ClipboardImageSource,
    command_runner: &impl ClipboardCommandRunner,
    wsl: bool,
) -> Result<ContentPart, ClipboardImageError> {
    match read_clipboard_png(source) {
        Ok(png_bytes) => png_bytes_to_content_part(&png_bytes),
        Err(ClipboardImageError::NoImage | ClipboardImageError::ClipboardUnavailable) if wsl => {
            read_wsl_fallback_image(command_runner)
        }
        Err(error) => Err(error),
    }
}

fn read_clipboard_png(source: &mut impl ClipboardImageSource) -> Result<Vec<u8>, ClipboardImageError> {
    let mut file_list_error = None;
    match source.file_paths() {
        Ok(paths) => {
            if let Some(png_bytes) = first_readable_image_file_as_png(paths) {
                return Ok(png_bytes);
            }
        }
        Err(error) => {
            file_list_error = Some(error);
        }
    }

    match source.rgba_image() {
        Ok(image) => encode_rgba_png(image.width, image.height, &image.bytes),
        Err(ClipboardImageError::NoImage) => match file_list_error {
            Some(ClipboardImageError::ClipboardUnavailable) => Err(ClipboardImageError::ClipboardUnavailable),
            Some(error) if error != ClipboardImageError::NoImage => Err(error),
            _ => Err(ClipboardImageError::NoImage),
        },
        Err(error) => Err(error),
    }
}

fn first_readable_image_file_as_png(paths: Vec<PathBuf>) -> Option<Vec<u8>> {
    paths
        .into_iter()
        .find_map(|path| image::open(path).ok().and_then(|image| encode_dynamic_image_png(&image).ok()))
}

fn encode_rgba_png(width: usize, height: usize, bytes: &[u8]) -> Result<Vec<u8>, ClipboardImageError> {
    let width_u32 = u32::try_from(width).map_err(|_| ClipboardImageError::NoImage)?;
    let height_u32 = u32::try_from(height).map_err(|_| ClipboardImageError::NoImage)?;
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(width_u32, height_u32, bytes.to_vec())
        .ok_or(ClipboardImageError::NoImage)?;
    encode_dynamic_image_png(&DynamicImage::ImageRgba8(buffer))
}

fn encode_dynamic_image_png(image: &DynamicImage) -> Result<Vec<u8>, ClipboardImageError> {
    let mut cursor = Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|_| ClipboardImageError::NoImage)?;
    Ok(cursor.into_inner())
}

fn png_bytes_to_content_part(png_bytes: &[u8]) -> Result<ContentPart, ClipboardImageError> {
    image::load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .map_err(|_| ClipboardImageError::WslFallbackFailure)?;
    Ok(ContentPart::image(BASE64.encode(png_bytes), PNG_MEDIA_TYPE))
}

fn read_wsl_fallback_image(command_runner: &impl ClipboardCommandRunner) -> Result<ContentPart, ClipboardImageError> {
    let attempts: [(&str, &[&str]); 2] = [
        (
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Sta",
                "-Command",
                POWERSHELL_CLIPBOARD_SCRIPT,
            ],
        ),
        ("pwsh.exe", &["-NoProfile", "-NonInteractive", "-Command", POWERSHELL_CLIPBOARD_SCRIPT]),
    ];

    for (program, args) in attempts {
        let output = match command_runner.run(program, args, WSL_FALLBACK_TIMEOUT) {
            Ok(output) => output,
            Err(_) => continue,
        };
        if output.timed_out {
            continue;
        }
        if output.status_code == Some(2) {
            return Err(ClipboardImageError::NoImage);
        }
        if output.status_code == Some(0) {
            let stdout = String::from_utf8(output.stdout).map_err(|_| ClipboardImageError::WslFallbackFailure)?;
            let png_bytes = BASE64
                .decode(stdout.trim())
                .map_err(|_| ClipboardImageError::WslFallbackFailure)?;
            if png_bytes.is_empty() {
                return Err(ClipboardImageError::WslFallbackFailure);
            }
            return png_bytes_to_content_part(&png_bytes);
        }
    }

    Err(ClipboardImageError::WslFallbackFailure)
}

fn is_wsl() -> bool {
    if env::var_os("WSL_DISTRO_NAME").is_some() {
        return true;
    }
    fs::read_to_string("/proc/version")
        .map(|version| version.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "android"))]
struct ArboardClipboardSource {
    clipboard: arboard::Clipboard,
}

#[cfg(not(target_os = "android"))]
impl ArboardClipboardSource {
    fn new() -> Result<Self, ClipboardImageError> {
        arboard::Clipboard::new()
            .map(|clipboard| Self { clipboard })
            .map_err(map_arboard_error)
    }
}

#[cfg(not(target_os = "android"))]
impl ClipboardImageSource for ArboardClipboardSource {
    fn file_paths(&mut self) -> Result<Vec<PathBuf>, ClipboardImageError> {
        self.clipboard.get().file_list().map_err(map_arboard_error)
    }

    fn rgba_image(&mut self) -> Result<ClipboardRgbaImage, ClipboardImageError> {
        let image = self.clipboard.get_image().map_err(map_arboard_error)?;
        Ok(ClipboardRgbaImage {
            width: image.width,
            height: image.height,
            bytes: image.bytes.into_owned(),
        })
    }
}

#[cfg(not(target_os = "android"))]
fn map_arboard_error(error: arboard::Error) -> ClipboardImageError {
    match error {
        arboard::Error::ContentNotAvailable | arboard::Error::ConversionFailure => ClipboardImageError::NoImage,
        arboard::Error::ClipboardNotSupported | arboard::Error::ClipboardOccupied | arboard::Error::Unknown { .. } => {
            ClipboardImageError::ClipboardUnavailable
        }
        _ => ClipboardImageError::ClipboardUnavailable,
    }
}

struct SystemCommandRunner;

impl ClipboardCommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> std::io::Result<CommandRunOutput> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("missing stdout pipe"))?;
        let (stdout_tx, stdout_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut stdout_bytes = Vec::new();
            let result = stdout.read_to_end(&mut stdout_bytes).map(|_| stdout_bytes);
            let _ = stdout_tx.send(result);
        });

        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                let stdout = stdout_rx.recv().unwrap_or_else(|_| Ok(Vec::new())).unwrap_or_default();
                return Ok(CommandRunOutput {
                    status_code: status.code(),
                    stdout,
                    timed_out: false,
                });
            }

            if start.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = stdout_rx.recv().unwrap_or_else(|_| Ok(Vec::new())).unwrap_or_default();
                return Ok(CommandRunOutput { status_code: None, stdout, timed_out: true });
            }

            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, fs, path::PathBuf};

    use super::*;

    #[derive(Default)]
    struct MockClipboardSource {
        file_paths_result: Option<Result<Vec<PathBuf>, ClipboardImageError>>,
        rgba_result: Option<Result<ClipboardRgbaImage, ClipboardImageError>>,
    }

    impl ClipboardImageSource for MockClipboardSource {
        fn file_paths(&mut self) -> Result<Vec<PathBuf>, ClipboardImageError> {
            self.file_paths_result.take().unwrap_or(Err(ClipboardImageError::NoImage))
        }

        fn rgba_image(&mut self) -> Result<ClipboardRgbaImage, ClipboardImageError> {
            self.rgba_result.take().unwrap_or(Err(ClipboardImageError::NoImage))
        }
    }

    #[derive(Default)]
    struct MockCommandRunner {
        outputs: std::sync::Mutex<VecDeque<std::io::Result<CommandRunOutput>>>,
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl MockCommandRunner {
        fn with_outputs(outputs: Vec<std::io::Result<CommandRunOutput>>) -> Self {
            Self {
                outputs: std::sync::Mutex::new(outputs.into()),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn called_programs(&self) -> Vec<String> {
            self.calls.lock().expect("mock command calls should not be poisoned").clone()
        }
    }

    impl ClipboardCommandRunner for MockCommandRunner {
        fn run(&self, program: &str, _args: &[&str], _timeout: Duration) -> std::io::Result<CommandRunOutput> {
            self.calls
                .lock()
                .expect("mock command calls should not be poisoned")
                .push(program.to_owned());
            self.outputs
                .lock()
                .expect("mock command outputs should not be poisoned")
                .pop_front()
                .unwrap_or_else(|| Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing mock command")))
        }
    }

    fn temp_file(name: &str) -> PathBuf {
        let mut path = env::temp_dir();
        path.push(format!("vtcode-clipboard-image-{}-{name}", std::process::id()));
        path
    }

    fn tiny_rgba() -> ClipboardRgbaImage {
        ClipboardRgbaImage {
            width: 2,
            height: 1,
            bytes: vec![255, 0, 0, 255, 0, 255, 0, 255],
        }
    }

    fn decoded_content_part_png(part: ContentPart) -> Vec<u8> {
        match part {
            ContentPart::Image { data, media_type } => {
                assert_eq!(media_type, PNG_MEDIA_TYPE);
                BASE64.decode(data).expect("image data should be base64")
            }
            ContentPart::Text { .. } => panic!("expected image content part"),
        }
    }

    fn assert_png_decodes(png_bytes: &[u8]) {
        image::load_from_memory_with_format(png_bytes, ImageFormat::Png).expect("PNG bytes should decode");
    }

    #[test]
    fn encodes_png_from_small_rgba_buffer() {
        let image = tiny_rgba();
        let png = encode_rgba_png(image.width, image.height, &image.bytes).expect("RGBA buffer should encode as PNG");

        assert_png_decodes(&png);
    }

    #[test]
    fn decodes_file_path_image_as_png() {
        let path = temp_file("single.png");
        let image = tiny_rgba();
        let png = encode_rgba_png(image.width, image.height, &image.bytes).expect("RGBA buffer should encode as PNG");
        fs::write(&path, &png).expect("write generated PNG");

        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(vec![path.clone()])),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };
        let runner = MockCommandRunner::default();

        let part = read_clipboard_image_with(&mut source, &runner, false).expect("file path image should be read");
        let decoded = decoded_content_part_png(part);
        assert_png_decodes(&decoded);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn file_list_chooses_first_readable_image_and_ignores_later_entries() {
        let missing = temp_file("missing.png");
        let first = temp_file("first.png");
        let later = temp_file("later.png");

        let first_png = encode_rgba_png(1, 1, &[1, 2, 3, 255]).expect("first PNG should encode");
        let later_png = encode_rgba_png(1, 1, &[9, 8, 7, 255]).expect("later PNG should encode");
        fs::write(&first, &first_png).expect("write first PNG");
        fs::write(&later, &later_png).expect("write later PNG");

        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(vec![missing, first.clone(), later.clone()])),
            rgba_result: Some(Ok(tiny_rgba())),
        };
        let runner = MockCommandRunner::default();

        let part = read_clipboard_image_with(&mut source, &runner, false).expect("first readable file should be used");
        assert_eq!(decoded_content_part_png(part), first_png);

        let _ = fs::remove_file(first);
        let _ = fs::remove_file(later);
    }

    #[test]
    fn converts_raw_clipboard_image_when_no_file_image_exists() {
        let image = tiny_rgba();
        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(Vec::new())),
            rgba_result: Some(Ok(image)),
        };
        let runner = MockCommandRunner::default();

        let part =
            read_clipboard_image_with(&mut source, &runner, false).expect("raw clipboard image should be converted");
        let decoded = decoded_content_part_png(part);

        assert_png_decodes(&decoded);
    }

    #[test]
    fn maps_no_image_without_wsl_fallback() {
        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(Vec::new())),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };
        let runner = MockCommandRunner::default();

        let error =
            read_clipboard_image_with(&mut source, &runner, false).expect_err("empty clipboard should map to no image");

        assert_eq!(error, ClipboardImageError::NoImage);
        assert!(runner.called_programs().is_empty());
    }

    #[test]
    fn maps_clipboard_unavailable_without_wsl_fallback() {
        let mut source = MockClipboardSource {
            file_paths_result: Some(Err(ClipboardImageError::ClipboardUnavailable)),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };
        let runner = MockCommandRunner::default();

        let error = read_clipboard_image_with(&mut source, &runner, false)
            .expect_err("unavailable clipboard should map distinctly");

        assert_eq!(error, ClipboardImageError::ClipboardUnavailable);
        assert!(runner.called_programs().is_empty());
    }

    #[test]
    fn wsl_fallback_success_returns_png_content_part() {
        let png = encode_rgba_png(1, 1, &[1, 2, 3, 255]).expect("PNG should encode");
        let runner = MockCommandRunner::with_outputs(vec![Ok(CommandRunOutput {
            status_code: Some(0),
            stdout: BASE64.encode(&png).into_bytes(),
            timed_out: false,
        })]);
        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(Vec::new())),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };

        let part = read_clipboard_image_with(&mut source, &runner, true).expect("WSL fallback should provide image");

        assert_eq!(decoded_content_part_png(part), png);
        assert_eq!(runner.called_programs(), vec!["powershell.exe"]);
    }

    #[test]
    fn wsl_fallback_no_image_exit_maps_to_no_image() {
        let runner = MockCommandRunner::with_outputs(vec![Ok(CommandRunOutput {
            status_code: Some(2),
            stdout: Vec::new(),
            timed_out: false,
        })]);
        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(Vec::new())),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };

        let error =
            read_clipboard_image_with(&mut source, &runner, true).expect_err("WSL no-image exit should stay no-image");

        assert_eq!(error, ClipboardImageError::NoImage);
    }

    #[test]
    fn wsl_fallback_failures_try_pwsh_then_map_to_failure() {
        let runner = MockCommandRunner::with_outputs(vec![
            Ok(CommandRunOutput {
                status_code: Some(1),
                stdout: Vec::new(),
                timed_out: false,
            }),
            Ok(CommandRunOutput {
                status_code: Some(0),
                stdout: b"not-base64".to_vec(),
                timed_out: false,
            }),
        ]);
        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(Vec::new())),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };

        let error =
            read_clipboard_image_with(&mut source, &runner, true).expect_err("invalid WSL fallback output should fail");

        assert_eq!(error, ClipboardImageError::WslFallbackFailure);
        assert_eq!(runner.called_programs(), vec!["powershell.exe", "pwsh.exe"]);
    }

    #[test]
    fn wsl_fallback_timeout_maps_to_failure() {
        let runner = MockCommandRunner::with_outputs(vec![Ok(CommandRunOutput {
            status_code: None,
            stdout: Vec::new(),
            timed_out: true,
        })]);
        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(Vec::new())),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };

        let error =
            read_clipboard_image_with(&mut source, &runner, true).expect_err("timed out WSL fallback should fail");

        assert_eq!(error, ClipboardImageError::WslFallbackFailure);
    }

    #[test]
    fn wsl_fallback_missing_commands_map_to_failure() {
        let runner = MockCommandRunner::default();
        let mut source = MockClipboardSource {
            file_paths_result: Some(Ok(Vec::new())),
            rgba_result: Some(Err(ClipboardImageError::NoImage)),
        };

        let error = read_clipboard_image_with(&mut source, &runner, true)
            .expect_err("missing WSL fallback commands should fail");

        assert_eq!(error, ClipboardImageError::WslFallbackFailure);
        assert_eq!(runner.called_programs(), vec!["powershell.exe", "pwsh.exe"]);
    }
}
