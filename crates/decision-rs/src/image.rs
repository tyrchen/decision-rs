//! Validated image evidence. Media types are sniffed from bytes, never trusted.

use std::{fmt, sync::Arc};

use sha2::Digest as _;
pub(crate) use sha2::Sha256;

use crate::error::Error;

/// Maximum images attached to one input.
pub const MAX_IMAGES: usize = 16;
/// Maximum encoded bytes of one image.
pub const MAX_IMAGE_BYTES: usize = 8 << 20;
/// Maximum encoded bytes of all images attached to one input.
pub const MAX_TOTAL_IMAGE_BYTES: usize = 16 << 20;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG_SIGNATURE: &[u8] = b"\xff\xd8\xff";

/// Image format detected from the leading bytes.
///
/// ```
/// use decision_rs::{Image, ImageMediaType};
/// let image = Image::try_from(&b"\xff\xd8\xff\xe0 jpeg payload"[..])?;
/// assert_eq!(image.media_type(), ImageMediaType::Jpeg);
/// assert_eq!(image.media_type().as_str(), "image/jpeg");
/// # Ok::<(), decision_rs::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageMediaType {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// `WebP` (RIFF container).
    Webp,
}
impl ImageMediaType {
    /// The IANA media type sent to providers.
    ///
    /// ```
    /// assert_eq!(decision_rs::ImageMediaType::Webp.as_str(), "image/webp");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
    fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(PNG_SIGNATURE) {
            Some(Self::Png)
        } else if bytes.starts_with(JPEG_SIGNATURE) {
            Some(Self::Jpeg)
        } else if bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            Some(Self::Webp)
        } else {
            None
        }
    }
    const fn tag(self) -> u8 {
        match self {
            Self::Png => 1,
            Self::Jpeg => 2,
            Self::Webp => 3,
        }
    }
}
impl fmt::Display for ImageMediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Requested visual detail; trades fidelity against input tokens.
///
/// `Auto` sends no field and lets the provider choose. `Low` and `High` map to
/// `OpenAI` `detail` and Gemini `resolution`.
///
/// ```
/// assert_eq!(decision_rs::ImageDetail::default(), decision_rs::ImageDetail::Auto);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageDetail {
    /// Provider default.
    #[default]
    Auto,
    /// Fewer tokens, coarser view.
    Low,
    /// More tokens, finer view.
    High,
}
impl ImageDetail {
    pub(crate) const fn wire(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Low => Some("low"),
            Self::High => Some("high"),
        }
    }
    const fn tag(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Low => 1,
            Self::High => 2,
        }
    }
}

/// SHA-256 digest of evidence, suitable as a cache or deduplication key.
///
/// ```
/// let digest = decision_rs::Input::try_from("ticket")?.digest();
/// assert_eq!(digest.to_string().len(), 64);
/// # Ok::<(), decision_rs::Error>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest([u8; 32]);
impl Digest {
    /// Raw digest bytes.
    ///
    /// ```
    /// let digest = decision_rs::Input::try_from("ticket")?.digest();
    /// assert_eq!(digest.as_bytes().len(), 32);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub(crate) fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
    pub(crate) fn finish(hasher: Sha256) -> Self {
        Self(hasher.finalize().into())
    }
}
impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}
impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

/// One validated, immutable image. Clones share the bytes.
///
/// ```
/// use decision_rs::{Image, ImageDetail, ImageMediaType};
/// let png = b"\x89PNG\r\n\x1a\n rest of the file".to_vec();
/// let image = Image::try_from(png)?.with_detail(ImageDetail::Low);
/// assert_eq!(image.media_type(), ImageMediaType::Png);
/// assert_eq!(image.detail(), ImageDetail::Low);
/// # Ok::<(), decision_rs::Error>(())
/// ```
#[derive(Clone)]
pub struct Image {
    bytes: Arc<[u8]>,
    media_type: ImageMediaType,
    detail: ImageDetail,
    digest: Digest,
}
impl Image {
    fn new(bytes: Arc<[u8]>) -> Result<Self, Error> {
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            return Err(Error::InvalidInput(
                "image must be nonempty and at most 8 MiB",
            ));
        }
        let media_type = ImageMediaType::sniff(&bytes).ok_or(Error::InvalidInput(
            "image media type must be PNG, JPEG or WebP",
        ))?;
        let digest = Digest::of(&bytes);
        Ok(Self {
            bytes,
            media_type,
            detail: ImageDetail::Auto,
            digest,
        })
    }
    /// Request a visual detail level for this image.
    ///
    /// ```
    /// # let image = decision_rs::Image::try_from(&b"\xff\xd8\xff\xdb"[..])?;
    /// let image = image.with_detail(decision_rs::ImageDetail::High);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn with_detail(mut self, detail: ImageDetail) -> Self {
        self.detail = detail;
        self
    }
    /// Sniffed media type.
    ///
    /// ```
    /// # let image = decision_rs::Image::try_from(&b"\xff\xd8\xff\xdb"[..])?;
    /// assert_eq!(image.media_type().as_str(), "image/jpeg");
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn media_type(&self) -> ImageMediaType {
        self.media_type
    }
    /// Requested detail level.
    ///
    /// ```
    /// # let image = decision_rs::Image::try_from(&b"\xff\xd8\xff\xdb"[..])?;
    /// assert_eq!(image.detail(), decision_rs::ImageDetail::Auto);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn detail(&self) -> ImageDetail {
        self.detail
    }
    /// Encoded image bytes exactly as sent.
    ///
    /// ```
    /// # let image = decision_rs::Image::try_from(&b"\xff\xd8\xff\xdb"[..])?;
    /// assert_eq!(image.as_bytes(), b"\xff\xd8\xff\xdb");
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Encoded size in bytes; never zero.
    ///
    /// ```
    /// # let image = decision_rs::Image::try_from(&b"\xff\xd8\xff\xdb"[..])?;
    /// assert_eq!(image.byte_len(), 4);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
    /// SHA-256 of the encoded bytes.
    ///
    /// ```
    /// # let image = decision_rs::Image::try_from(&b"\xff\xd8\xff\xdb"[..])?;
    /// let key = image.digest().to_string();
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn digest(&self) -> Digest {
        self.digest
    }
    /// Feed this image's identity (format, detail, content digest) into an input digest.
    pub(crate) fn digest_into(&self, hasher: &mut Sha256) {
        hasher.update([self.media_type.tag(), self.detail.tag()]);
        hasher.update(self.digest.as_bytes());
    }
}
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest && self.detail == other.detail && self.bytes == other.bytes
    }
}
impl Eq for Image {}
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("media_type", &self.media_type)
            .field("detail", &self.detail)
            .field("bytes", &self.bytes.len())
            .field("digest", &self.digest)
            .finish()
    }
}
impl TryFrom<Vec<u8>> for Image {
    type Error = Error;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(Error::InvalidInput(
                "image must be nonempty and at most 8 MiB",
            ));
        }
        Self::new(Arc::from(bytes))
    }
}
impl TryFrom<&[u8]> for Image {
    type Error = Error;
    fn try_from(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(Error::InvalidInput(
                "image must be nonempty and at most 8 MiB",
            ));
        }
        Self::new(Arc::from(bytes))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec(), ImageMediaType::Png)]
    #[case(b"\xff\xd8\xff\xe0\0\x10JFIF".to_vec(), ImageMediaType::Jpeg)]
    #[case(b"RIFF\x24\0\0\0WEBPVP8 ".to_vec(), ImageMediaType::Webp)]
    fn test_should_sniff_media_type_from_bytes(
        #[case] bytes: Vec<u8>,
        #[case] expected: ImageMediaType,
    ) {
        let image = Image::try_from(bytes).unwrap();
        assert_eq!(image.media_type(), expected);
        assert_eq!(image.detail(), ImageDetail::Auto);
    }

    #[rstest]
    #[case::empty(Vec::new())]
    #[case::gif(b"GIF89a\x01\0\x01\0".to_vec())]
    #[case::truncated_png(b"\x89PNG\r\n".to_vec())]
    #[case::truncated_webp(b"RIFF\0\0\0\0WEB".to_vec())]
    #[case::riff_not_webp(b"RIFF\0\0\0\0WAVEfmt ".to_vec())]
    #[case::text(b"data:image/png;base64,AAAA".to_vec())]
    fn test_should_reject_unknown_or_empty_images(#[case] bytes: Vec<u8>) {
        assert!(matches!(
            Image::try_from(bytes),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn test_should_reject_oversized_image() {
        let mut bytes = vec![0_u8; MAX_IMAGE_BYTES + 1];
        bytes[..3].copy_from_slice(JPEG_SIGNATURE);
        assert!(matches!(
            Image::try_from(bytes.as_slice()),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            Image::try_from(bytes),
            Err(Error::InvalidInput(_))
        ));
        let mut bytes = vec![0_u8; MAX_IMAGE_BYTES];
        bytes[..3].copy_from_slice(JPEG_SIGNATURE);
        assert_eq!(Image::try_from(bytes).unwrap().byte_len(), MAX_IMAGE_BYTES);
    }

    #[test]
    fn test_should_digest_bytes_with_known_sha256() {
        let image = Image::try_from(&b"\xff\xd8\xff"[..]).unwrap();
        assert_eq!(image.digest(), Digest::of(b"\xff\xd8\xff"));
        assert_eq!(
            Digest::of(b"").to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_should_redact_bytes_in_debug() {
        let image = Image::try_from(&b"\xff\xd8\xffSECRET-PIXELS"[..]).unwrap();
        let debug = format!("{image:?}");
        assert!(debug.contains("Jpeg"));
        assert!(debug.contains("bytes: 16"));
        assert!(!debug.contains("SECRET"));
    }

    #[test]
    fn test_should_compare_detail_and_bytes() {
        let a = Image::try_from(&b"\xff\xd8\xffa"[..]).unwrap();
        let b = Image::try_from(&b"\xff\xd8\xffb"[..]).unwrap();
        assert_eq!(a, a.clone());
        assert_ne!(a, b);
        assert_ne!(a, a.clone().with_detail(ImageDetail::Low));
    }
}
