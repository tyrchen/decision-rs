//! Validated, immutable evidence shared by every backend: text plus optional images.

use std::{fmt, sync::Arc};

use serde::Serialize;
use serde_json::Value;
use sha2::Digest as _;

use crate::{
    error::Error,
    image::{Digest, Image, MAX_IMAGES, MAX_TOTAL_IMAGE_BYTES, Sha256},
    json,
};

const DIGEST_DOMAIN: &[u8] = b"decision-rs/input/v1";

/// Maximum input length in UTF-8 bytes.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// Prepared evidence: required text, then zero or more images in attachment order.
///
/// JSON evidence is sorted compact text on every backend. Images are accepted by
/// the `OpenAI` and Gemini protocols; every other protocol returns
/// `Error::ImagesUnsupported` before any network call.
///
/// ```
/// let input = decision_rs::Input::try_from("a support ticket")?;
/// assert_eq!(input.as_str(), "a support ticket");
/// assert!(input.images().is_empty());
/// # Ok::<(), decision_rs::Error>(())
/// ```
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Input {
    text: Arc<str>,
    images: Arc<[Image]>,
}
impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input")
            .field("bytes", &self.text.len())
            .field("images", &self.images.len())
            .finish_non_exhaustive()
    }
}
impl Input {
    /// Serialize an object or array as bounded, canonical JSON evidence.
    ///
    /// # Errors
    /// Rejects scalars, excessive nesting, strings, nodes, or encoded size.
    ///
    /// ```
    /// let input = decision_rs::Input::json(&serde_json::json!({"z": 2, "a": 1}))?;
    /// assert_eq!(input.as_str(), r#"{"a":1,"z":2}"#);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    pub fn json<T: Serialize>(value: &T) -> Result<Self, Error> {
        let bytes = json::encode(value, MAX_INPUT_BYTES)?;
        let mut value = json::parse(&bytes, 256 * 1024, 256)
            .map_err(|_| Error::InvalidInput("JSON evidence exceeds structural limits"))?;
        if !matches!(value, Value::Object(_) | Value::Array(_)) {
            return Err(Error::InvalidInput(
                "JSON evidence must be an object or array",
            ));
        }
        value.sort_all_objects();
        let text = serde_json::to_string(&value)
            .map_err(|_| Error::InvalidInput("JSON encoding failed"))?;
        Self::try_from(text)
    }
    /// Borrow the exact evidence text sent to every provider.
    ///
    /// ```no_run
    /// # fn example(value: &decision_rs::Input) -> Result<(), decision_rs::Error> {
    /// let evidence = value.as_str();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
    /// Attach one image after the text and any earlier images.
    ///
    /// # Errors
    /// Returns `InvalidInput` beyond `MAX_IMAGES` images or `MAX_TOTAL_IMAGE_BYTES`.
    ///
    /// ```
    /// use decision_rs::{Image, Input};
    /// let frame = Image::try_from(&b"\x89PNG\r\n\x1a\n frame bytes"[..])?;
    /// let input = Input::try_from("Narration: revenue doubled.")?.with_image(frame)?;
    /// assert_eq!(input.images().len(), 1);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    pub fn with_image(self, image: Image) -> Result<Self, Error> {
        self.with_images([image])
    }
    /// Attach images in iteration order after the text and any earlier images.
    ///
    /// # Errors
    /// Returns `InvalidInput` beyond `MAX_IMAGES` images or `MAX_TOTAL_IMAGE_BYTES`.
    ///
    /// ```
    /// use decision_rs::{Image, Input};
    /// let before = Image::try_from(&b"\xff\xd8\xff before"[..])?;
    /// let after = Image::try_from(&b"\xff\xd8\xff after"[..])?;
    /// let input = Input::try_from("Did the layout change?")?.with_images([before, after])?;
    /// assert_eq!(input.images().len(), 2);
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    pub fn with_images(self, images: impl IntoIterator<Item = Image>) -> Result<Self, Error> {
        let mut all = self.images.to_vec();
        for image in images {
            if all.len() >= MAX_IMAGES {
                return Err(Error::InvalidInput("at most 16 images per input"));
            }
            all.push(image);
        }
        let total = all
            .iter()
            .try_fold(0_usize, |sum, image| sum.checked_add(image.byte_len()));
        if total.is_none_or(|total| total > MAX_TOTAL_IMAGE_BYTES) {
            return Err(Error::InvalidInput("images exceed 16 MiB per input"));
        }
        Ok(Self {
            text: self.text,
            images: all.into(),
        })
    }
    /// Attached images in the order they are sent.
    ///
    /// ```
    /// let input = decision_rs::Input::try_from("text only")?;
    /// assert!(input.images().is_empty());
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn images(&self) -> &[Image] {
        &self.images
    }
    /// SHA-256 over the text and every image (format, detail, content), in order.
    ///
    /// Use it as the cache, deduplication or idempotency key for this evidence:
    /// equal text with different images always yields a different digest.
    ///
    /// ```
    /// use decision_rs::{Image, Input};
    /// let text = Input::try_from("Narration: revenue doubled.")?;
    /// let frame = Image::try_from(&b"\xff\xd8\xff frame"[..])?;
    /// let with_frame = text.clone().with_image(frame)?;
    /// assert_ne!(text.digest(), with_frame.digest());
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    #[must_use]
    pub fn digest(&self) -> Digest {
        let mut hasher = Sha256::new();
        hasher.update(DIGEST_DOMAIN);
        hasher.update((self.text.len() as u64).to_be_bytes());
        hasher.update(self.text.as_bytes());
        hasher.update((self.images.len() as u64).to_be_bytes());
        for image in self.images.iter() {
            image.digest_into(&mut hasher);
        }
        Digest::finish(hasher)
    }
    /// Total encoded image bytes; bounded by `MAX_TOTAL_IMAGE_BYTES`.
    pub(crate) fn image_bytes(&self) -> usize {
        self.images.iter().map(Image::byte_len).sum()
    }
}
impl TryFrom<&str> for Input {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self, Error> {
        if value.trim().is_empty() || value.len() > MAX_INPUT_BYTES {
            return Err(Error::InvalidInput(
                "input must be nonempty and at most 1 MiB",
            ));
        }
        Ok(Self {
            text: Arc::from(value),
            images: Arc::from([]),
        })
    }
}
impl TryFrom<String> for Input {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        Self::try_from(value.as_str())
    }
}
mod sealed {
    pub trait Sealed {}
    impl Sealed for &str {}
    impl Sealed for String {}
    impl Sealed for super::Input {}
    impl Sealed for &super::Input {}
}
/// Supported fallible conversions into prepared evidence.
pub trait IntoInput: sealed::Sealed {
    /// Validate or cheaply share evidence.
    ///
    /// # Errors
    /// Returns the validation errors documented by Input.
    /// ```
    /// use decision_rs::IntoInput;
    /// let input = "A support ticket".try_into_input()?;
    /// assert_eq!(input.as_str(), "A support ticket");
    /// # Ok::<(), decision_rs::Error>(())
    /// ```
    fn try_into_input(self) -> Result<Input, Error>;
}
impl IntoInput for &str {
    fn try_into_input(self) -> Result<Input, Error> {
        Input::try_from(self)
    }
}
impl IntoInput for String {
    fn try_into_input(self) -> Result<Input, Error> {
        Input::try_from(self)
    }
}
impl IntoInput for Input {
    fn try_into_input(self) -> Result<Input, Error> {
        Ok(self)
    }
}
impl IntoInput for &Input {
    fn try_into_input(self) -> Result<Input, Error> {
        Ok(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageDetail, image::MAX_IMAGE_BYTES};

    fn jpeg(tag: u8, len: usize) -> Image {
        let mut bytes = vec![tag; len.max(4)];
        bytes[..3].copy_from_slice(b"\xff\xd8\xff");
        Image::try_from(bytes).unwrap()
    }

    #[test]
    fn test_should_bound_image_count() {
        let images: Vec<_> = (0..=MAX_IMAGES).map(|i| jpeg(i as u8, 8)).collect();
        let input = Input::try_from("frames").unwrap();
        let full = input
            .clone()
            .with_images(images[..MAX_IMAGES].iter().cloned())
            .unwrap();
        assert_eq!(full.images().len(), MAX_IMAGES);
        assert!(matches!(
            full.with_image(images[MAX_IMAGES].clone()),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            input.with_images(images),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn test_should_bound_total_image_bytes() {
        let input = Input::try_from("frames").unwrap();
        let at_limit = input
            .clone()
            .with_images([jpeg(1, MAX_IMAGE_BYTES), jpeg(2, MAX_IMAGE_BYTES)])
            .unwrap();
        assert_eq!(at_limit.image_bytes(), MAX_TOTAL_IMAGE_BYTES);
        assert!(matches!(
            at_limit.with_image(jpeg(3, 4)),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn test_should_digest_text_images_order_and_detail() {
        let text = Input::try_from("frame").unwrap();
        let (a, b) = (jpeg(1, 8), jpeg(2, 8));
        let ab = text.clone().with_images([a.clone(), b.clone()]).unwrap();
        let ba = text.clone().with_images([b.clone(), a.clone()]).unwrap();
        let low = text
            .clone()
            .with_images([a.clone().with_detail(ImageDetail::Low), b.clone()])
            .unwrap();
        let other_text = Input::try_from("frame!")
            .unwrap()
            .with_images([a.clone(), b.clone()])
            .unwrap();
        let digests = [
            text.digest(),
            ab.digest(),
            ba.digest(),
            low.digest(),
            other_text.digest(),
        ];
        for (i, left) in digests.iter().enumerate() {
            for right in &digests[i + 1..] {
                assert_ne!(left, right);
            }
        }
        assert_eq!(ab.digest(), text.with_images([a, b]).unwrap().digest());
        assert_ne!(ab, ba);
    }

    #[test]
    fn test_should_keep_text_digest_stable() {
        // Pinned so cache keys survive SDK upgrades; change only with a new domain tag.
        assert_eq!(
            Input::try_from("ticket").unwrap().digest().to_string(),
            STABLE_TEXT_DIGEST
        );
    }
    const STABLE_TEXT_DIGEST: &str =
        "42630ab2c7d9f1959387a77b927be56ab16a0c37148bffef85a55ef38856355a";

    #[test]
    fn test_should_describe_images_without_bytes_in_debug() {
        let input = Input::try_from("frame")
            .unwrap()
            .with_image(jpeg(b'Z', 32))
            .unwrap();
        let debug = format!("{input:?}");
        assert!(debug.contains("images: 1"));
        assert!(!debug.contains("ZZZZ"));
    }
}
