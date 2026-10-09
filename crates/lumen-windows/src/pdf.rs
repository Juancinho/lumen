//! On-demand PDF rasterization by the Windows OS, independent of the presentation shell.
//! Call on one background MTA thread; never on a query or UI thread (ADR-040).

use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::Duration;
use std::time::SystemTime;
use std::{collections::VecDeque, io::Read, sync::Arc};

pub const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_PAGES: u32 = 512;
pub const MAX_EDGE: u32 = 960;
pub const MAX_PNG_BYTES: usize = 6 * 1024 * 1024;
const CACHE_BYTES: usize = 12 * 1024 * 1024;
const CACHE_PAGES: usize = 4;
#[cfg(windows)]
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageImage {
    pub page_number: u32,
    pub page_count: u32,
    pub width: u32,
    pub height: u32,
    pub png: Arc<[u8]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewError {
    Unavailable,
    TooLarge,
    PageLimit,
    InvalidPage,
    InvalidSize,
    Encrypted,
    Changed,
    Cancelled,
    Timeout,
    Decode,
}

impl PreviewError {
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::TooLarge => "This PDF exceeds the 16 MB preview limit",
            Self::PageLimit => "This PDF exceeds the 512-page preview limit",
            Self::InvalidPage => "This page is no longer available",
            Self::Encrypted => "Password-protected PDFs cannot be previewed",
            Self::Changed => "The PDF changed; select it again to refresh",
            Self::Timeout => "This PDF took too long to preview",
            Self::Cancelled => "Preview cancelled",
            _ => "Page preview is unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Source {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}

fn source(path: &Path) -> Result<Source, PreviewError> {
    let meta = std::fs::metadata(path).map_err(|_| PreviewError::Unavailable)?;
    if !meta.is_file() {
        return Err(PreviewError::Unavailable);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Do not hydrate offline/recall-on-open/recall-on-data cloud placeholders.
        if meta.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0 {
            return Err(PreviewError::Unavailable);
        }
    }
    if meta.len() > MAX_SOURCE_BYTES {
        return Err(PreviewError::TooLarge);
    }
    Ok(Source {
        path: path.to_owned(),
        size: meta.len(),
        modified: meta.modified().map_err(|_| PreviewError::Unavailable)?,
        created: meta.created().ok(),
    })
}

fn read_source(key: &Source) -> Result<Vec<u8>, PreviewError> {
    let mut bytes = Vec::new();
    std::fs::File::open(&key.path)
        .map_err(|_| PreviewError::Unavailable)?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PreviewError::Unavailable)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(PreviewError::TooLarge);
    }
    if bytes.len() as u64 != key.size || source(&key.path)? != *key {
        return Err(PreviewError::Changed);
    }
    Ok(bytes)
}

#[cfg(any(windows, test))]
fn dimensions(width: f32, height: f32) -> Result<(u32, u32), PreviewError> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(PreviewError::InvalidSize);
    }
    let scale = MAX_EDGE as f32 / width.max(height);
    Ok((
        (width * scale).round().clamp(1.0, MAX_EDGE as f32) as u32,
        (height * scale).round().clamp(1.0, MAX_EDGE as f32) as u32,
    ))
}

/// One source document and at most four/12 MiB raster pages, memory only.
#[derive(Default)]
pub struct Renderer {
    source: Option<Source>,
    engine: platform::Engine,
    pages: VecDeque<PageImage>,
    // WinRT initialization/uninitialization and object lifetime belong to one thread.
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Renderer {
    pub fn clear(&mut self) {
        self.source = None;
        self.engine = platform::Engine::default();
        self.pages.clear();
    }

    /// A physical one-based page. Cancellation/deadline request OS async cancellation;
    /// they are not a hard native heap or execution sandbox.
    ///
    /// # Errors
    /// Unsupported, changed, encrypted, malformed or over-limit sources/pages.
    pub fn render(
        &mut self,
        path: &Path,
        page: u32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PageImage, PreviewError> {
        if cancelled() {
            return Err(PreviewError::Cancelled);
        }
        if !(1..=MAX_PAGES).contains(&page) {
            return Err(PreviewError::InvalidPage);
        }
        let key = source(path)?;
        if self.source.as_ref() != Some(&key) {
            self.clear();
            let bytes = read_source(&key)?;
            self.engine.load(&bytes, cancelled)?;
            self.source = Some(key.clone());
        }
        if let Some(index) = self.pages.iter().position(|p| p.page_number == page) {
            let image = self.pages.remove(index).ok_or(PreviewError::Decode)?;
            self.pages.push_back(image.clone());
            return Ok(image);
        }
        let image = self.engine.render(page, cancelled)?;
        if source(path)? != key {
            self.clear();
            return Err(PreviewError::Changed);
        }
        if cancelled() {
            return Err(PreviewError::Cancelled);
        }
        while !self.pages.is_empty()
            && (self.pages.len() >= CACHE_PAGES
                || self.pages.iter().map(|p| p.png.len()).sum::<usize>() + image.png.len()
                    > CACHE_BYTES)
        {
            self.pages.pop_front();
        }
        self.pages.push_back(image.clone());
        Ok(image)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // WinRT apartment initialization, balanced on the same worker.
mod platform {
    use super::*;
    use std::time::Instant;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

    // Status is inspected only while doing requested work. No hidden/idle polling.
    macro_rules! wait {
        ($op:expr, $cancel:expr, $started:expr) => {{
            let op = $op.map_err(|_| PreviewError::Decode)?;
            loop {
                if $cancel() || $started.elapsed() > DEADLINE {
                    let _ = op.Cancel();
                    return Err(if $cancel() {
                        PreviewError::Cancelled
                    } else {
                        PreviewError::Timeout
                    });
                }
                if op.Status().map_err(|_| PreviewError::Decode)?.0 != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            op.GetResults().map_err(|error| {
                if error.code().0 as u32 == 0x8007052b {
                    PreviewError::Encrypted
                } else {
                    PreviewError::Decode
                }
            })?
        }};
    }

    #[derive(Default)]
    pub(super) struct Engine {
        doc: Option<PdfDocument>,
        apartment: bool,
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            self.doc = None;
            if self.apartment {
                // SAFETY: paired with this engine's successful RoInitialize on this thread.
                unsafe { RoUninitialize() };
            }
        }
    }

    impl Engine {
        pub(super) fn load(
            &mut self,
            bytes: &[u8],
            cancel: &dyn Fn() -> bool,
        ) -> Result<(), PreviewError> {
            if !self.apartment {
                // SAFETY: engine belongs to one background thread; no STA/UI thread use.
                unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
                    .map_err(|_| PreviewError::Unavailable)?;
                self.apartment = true;
            }
            let started = Instant::now();
            let stream =
                InMemoryRandomAccessStream::new().map_err(|_| PreviewError::Unavailable)?;
            let writer = DataWriter::CreateDataWriter(&stream).map_err(|_| PreviewError::Decode)?;
            writer.WriteBytes(bytes).map_err(|_| PreviewError::Decode)?;
            wait!(writer.StoreAsync(), cancel, started);
            writer.DetachStream().map_err(|_| PreviewError::Decode)?;
            stream.Seek(0).map_err(|_| PreviewError::Decode)?;
            let doc = wait!(PdfDocument::LoadFromStreamAsync(&stream), cancel, started);
            if doc
                .IsPasswordProtected()
                .map_err(|_| PreviewError::Decode)?
            {
                return Err(PreviewError::Encrypted);
            }
            let count = doc.PageCount().map_err(|_| PreviewError::Decode)?;
            if count == 0 || count > MAX_PAGES {
                return Err(PreviewError::PageLimit);
            }
            self.doc = Some(doc);
            Ok(())
        }

        pub(super) fn render(
            &self,
            number: u32,
            cancel: &dyn Fn() -> bool,
        ) -> Result<PageImage, PreviewError> {
            let started = Instant::now();
            let doc = self.doc.as_ref().ok_or(PreviewError::Unavailable)?;
            let count = doc.PageCount().map_err(|_| PreviewError::Decode)?;
            if number == 0 || number > count {
                return Err(PreviewError::InvalidPage);
            }
            let page = doc.GetPage(number - 1).map_err(|_| PreviewError::Decode)?;
            // IClosable: close the native page on every success/failure exit.
            struct ClosePage(windows::Data::Pdf::PdfPage);
            impl Drop for ClosePage {
                fn drop(&mut self) {
                    let _ = self.0.Close();
                }
            }
            let page = ClosePage(page);
            let size = page.0.Size().map_err(|_| PreviewError::Decode)?;
            let (width, height) = dimensions(size.Width, size.Height)?;
            let options = PdfPageRenderOptions::new().map_err(|_| PreviewError::Decode)?;
            options
                .SetDestinationWidth(width)
                .map_err(|_| PreviewError::Decode)?;
            options
                .SetDestinationHeight(height)
                .map_err(|_| PreviewError::Decode)?;
            // PNG encoder GUID defined by Windows.Graphics.Imaging.BitmapEncoder.PngEncoderId.
            options
                .SetBitmapEncoderId(windows_core::GUID::from_u128(
                    0x27949969_876a_41d7_9447_568f6a35a4dc,
                ))
                .map_err(|_| PreviewError::Decode)?;
            let stream = InMemoryRandomAccessStream::new().map_err(|_| PreviewError::Decode)?;
            wait!(
                page.0.RenderWithOptionsToStreamAsync(&stream, &options),
                cancel,
                started
            );
            let size = stream.Size().map_err(|_| PreviewError::Decode)?;
            if size > MAX_PNG_BYTES as u64 {
                return Err(PreviewError::TooLarge);
            }
            let reader = DataReader::CreateDataReader(
                &stream
                    .GetInputStreamAt(0)
                    .map_err(|_| PreviewError::Decode)?,
            )
            .map_err(|_| PreviewError::Decode)?;
            let read = wait!(reader.LoadAsync(size as u32), cancel, started);
            if u64::from(read) != size {
                return Err(PreviewError::Decode);
            }
            let mut png = vec![0; size as usize];
            reader
                .ReadBytes(&mut png)
                .map_err(|_| PreviewError::Decode)?;
            if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(PreviewError::Decode);
            }
            Ok(PageImage {
                page_number: number,
                page_count: count,
                width,
                height,
                png: png.into(),
            })
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    #[derive(Default)]
    pub(super) struct Engine;
    impl Engine {
        pub(super) fn load(&mut self, _: &[u8], _: &dyn Fn() -> bool) -> Result<(), PreviewError> {
            Err(PreviewError::Unavailable)
        }
        pub(super) fn render(
            &self,
            _: u32,
            _: &dyn Fn() -> bool,
        ) -> Result<PageImage, PreviewError> {
            Err(PreviewError::Unavailable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_and_admission_are_bounded() {
        assert_eq!(dimensions(612.0, 792.0), Ok((742, 960)));
        assert_eq!(dimensions(2000.0, 100.0), Ok((960, 48)));
        assert!(dimensions(f32::INFINITY, 10.0).is_err());
        assert!(dimensions(0.0, 10.0).is_err());
        let mut renderer = Renderer::default();
        assert_eq!(
            renderer.render(Path::new("missing.pdf"), 0, &|| false),
            Err(PreviewError::InvalidPage)
        );
        assert_eq!(
            renderer.render(Path::new("missing.pdf"), 1, &|| true),
            Err(PreviewError::Cancelled)
        );
    }
}
