//! Optional OS OCR, owned by one background MTA thread. No file reads or shell types.
use std::marker::PhantomData;
use std::rc::Rc;

pub const MAX_EDGE: u32 = 4096;
pub const MAX_PIXELS: u64 = 8_000_000;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Language,
    Dimensions,
    TextLimit,
    Cancelled,
    Timeout,
    Recognition,
}
impl Error {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "ocr:unavailable",
            Self::Language => "ocr:language",
            Self::Dimensions => "ocr:pixel_limit",
            Self::TextLimit => "ocr:text_limit",
            Self::Cancelled => "ocr:cancelled",
            Self::Timeout => "ocr:timeout",
            Self::Recognition => "ocr:recognition",
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct Text {
    pub text: String,
    pub language: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bad_dimensions_and_cancellation_do_not_initialize_the_native_engine() {
        let mut engine = Engine::default();
        assert!(matches!(
            engine.recognize(1, 1, &[0; 3], &|| true),
            Err(Error::Cancelled)
        ));
        assert!(matches!(
            engine.recognize(4097, 1, &[], &|| false),
            Err(Error::Dimensions)
        ));
        assert!(matches!(
            engine.recognize(4096, 4096, &[], &|| false),
            Err(Error::Dimensions)
        ));
        assert!(matches!(
            engine.recognize(100, 100, &[0; 3], &|| false),
            Err(Error::Dimensions)
        ));
        #[cfg(windows)]
        assert!(!engine.apartment);
    }
}

#[derive(Default)]
pub struct Engine {
    #[cfg(windows)]
    native: Option<windows::Media::Ocr::OcrEngine>,
    #[cfg(windows)]
    apartment: bool,
    // COM init/uninit and native requests belong to the thread that owns this engine.
    thread: PhantomData<Rc<()>>,
}

impl Engine {
    /// Installed profile language; initialization happens only on this background thread.
    /// # Errors
    /// Missing platform/engine/language.
    pub fn language(&mut self) -> Result<String, Error> {
        #[cfg(windows)]
        {
            self.init()?;
            self.native
                .as_ref()
                .ok_or(Error::Unavailable)?
                .RecognizerLanguage()
                .and_then(|l| l.LanguageTag())
                .map(|s| s.to_string())
                .map_err(|_| Error::Language)
        }
        #[cfg(not(windows))]
        {
            Err(Error::Unavailable)
        }
    }
    /// Bounded, EXIF-oriented RGB supplied by the domain image decoder.
    /// # Errors
    /// Invalid admission, cancellation, deadline or native recognition failure.
    pub fn recognize(
        &mut self,
        width: u32,
        height: u32,
        rgb: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Text, Error> {
        if cancel() {
            return Err(Error::Cancelled);
        }
        let pixels = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || width > MAX_EDGE
            || height > MAX_EDGE
            || pixels > MAX_PIXELS
            || usize::try_from(pixels * 3).ok() != Some(rgb.len())
        {
            return Err(Error::Dimensions);
        }
        #[cfg(windows)]
        {
            self.recognize_native(width, height, rgb, cancel)
        }
        #[cfg(not(windows))]
        {
            Err(Error::Unavailable)
        }
    }
}

#[cfg(windows)]
impl Engine {
    #[allow(unsafe_code)]
    fn init(&mut self) -> Result<(), Error> {
        use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
        if !self.apartment {
            // SAFETY: engine is !Send/!Sync, owned by a background MTA thread.
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|_| Error::Unavailable)?;
            self.apartment = true;
        }
        if self.native.is_none() {
            use windows::Media::Ocr::OcrEngine;
            if OcrEngine::AvailableRecognizerLanguages()
                .and_then(|l| l.Size())
                .map_err(|_| Error::Unavailable)?
                == 0
            {
                return Err(Error::Language);
            }
            self.native =
                Some(OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| Error::Language)?);
        }
        Ok(())
    }
    fn recognize_native(
        &mut self,
        width: u32,
        height: u32,
        rgb: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Text, Error> {
        use std::time::{Duration, Instant};
        use windows::{
            Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
            Media::Ocr::OcrEngine,
            Storage::Streams::DataWriter,
        };
        let language = self.language()?;
        if width > OcrEngine::MaxImageDimension().map_err(|_| Error::Unavailable)?
            || height > OcrEngine::MaxImageDimension().map_err(|_| Error::Unavailable)?
        {
            return Err(Error::Dimensions);
        }
        let gray: Vec<u8> = rgb
            .chunks_exact(3)
            .map(|p| {
                u8::try_from(
                    (77 * u32::from(p[0]) + 150 * u32::from(p[1]) + 29 * u32::from(p[2])) >> 8,
                )
                .unwrap_or(0)
            })
            .collect();
        let writer = DataWriter::new().map_err(|_| Error::Unavailable)?;
        writer.WriteBytes(&gray).map_err(|_| Error::Recognition)?;
        let buffer = writer.DetachBuffer().map_err(|_| Error::Recognition)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Gray8,
            i32::try_from(width).map_err(|_| Error::Dimensions)?,
            i32::try_from(height).map_err(|_| Error::Dimensions)?,
        )
        .map_err(|_| Error::Recognition)?;
        if cancel() {
            return Err(Error::Cancelled);
        }
        let op = self
            .native
            .as_ref()
            .ok_or(Error::Unavailable)?
            .RecognizeAsync(&bitmap)
            .map_err(|_| Error::Recognition)?;
        let started = Instant::now();
        loop {
            if cancel() || started.elapsed() > Duration::from_secs(5) {
                let _ = op.Cancel();
                return Err(if cancel() {
                    Error::Cancelled
                } else {
                    Error::Timeout
                });
            }
            if op.Status().map_err(|_| Error::Recognition)?.0 != 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let result = op.GetResults().map_err(|_| Error::Recognition)?;
        let mut text = String::new();
        for line in result.Lines().map_err(|_| Error::Recognition)? {
            if cancel() {
                return Err(Error::Cancelled);
            }
            let line = line.Text().map_err(|_| Error::Recognition)?.to_string();
            if text.len() + line.len() + 1 > MAX_TEXT_BYTES {
                return Err(Error::TextLimit);
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.extend(line.chars().filter(|c| !c.is_control() || *c == '\t'));
        }
        Ok(Text {
            text: text.trim().to_owned(),
            language,
        })
    }
}
#[cfg(windows)]
impl Drop for Engine {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        self.native = None;
        if self.apartment {
            // SAFETY: paired with successful RoInitialize on this same owning thread.
            unsafe { windows::Win32::System::WinRT::RoUninitialize() };
        }
    }
}
