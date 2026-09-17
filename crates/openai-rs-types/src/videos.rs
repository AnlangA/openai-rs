//! Deprecated Sora video API types. The service is scheduled to shut down on 2026-09-24.

use crate::responses::ResponseMisalignment;
use crate::{ExtraFields, Nullable, Omittable, ReplayableMultipartSource};
use serde::{Deserialize, Serialize};

crate::open_string_enum! {
    /// Official `VideoModel` string domain, with future values preserved.
    pub enum VideoModel {
        /// The Sora 2 video generation model.
        Sora2 = "sora-2",
        /// The Sora 2 Pro video generation model.
        Sora2Pro = "sora-2-pro",
        /// The October 6, 2025 snapshot of Sora 2.
        Sora2October2025 = "sora-2-2025-10-06",
        /// The October 6, 2025 snapshot of Sora 2 Pro.
        Sora2ProOctober2025 = "sora-2-pro-2025-10-06",
        /// The December 8, 2025 snapshot of Sora 2.
        Sora2December2025 = "sora-2-2025-12-08",
    }
}

crate::open_string_enum! {
    /// Official `VideoStatus` string domain, with future values preserved.
    pub enum VideoStatus {
        /// The job is waiting to begin generation.
        Queued = "queued",
        /// Video generation is underway.
        InProgress = "in_progress",
        /// Video generation finished successfully.
        Completed = "completed",
        /// Video generation failed.
        Failed = "failed",
    }
}

crate::open_string_enum! {
    /// Official `VideoSize` string domain, with future values preserved.
    pub enum VideoSize {
        /// Portrait output at 720 by 1280 pixels.
        Portrait = "720x1280",
        /// Landscape output at 1280 by 720 pixels.
        Landscape = "1280x720",
        /// Portrait output at 1024 by 1792 pixels.
        PortraitLarge = "1024x1792",
        /// Landscape output at 1792 by 1024 pixels.
        LandscapeLarge = "1792x1024",
    }
}

crate::open_string_enum! {
    /// Official `VideoSeconds` string domain, with future values preserved.
    pub enum VideoSeconds {
        /// Four seconds of video.
        Four = "4",
        /// Eight seconds of video.
        Eight = "8",
        /// Twelve seconds of video.
        Twelve = "12",
    }
}

crate::open_string_enum! {
    /// Official `VideoContentVariant` string domain, with future values preserved.
    pub enum VideoContentVariant {
        /// The generated video file.
        Video = "video",
        /// A thumbnail image for the video.
        Thumbnail = "thumbnail",
        /// A sheet of preview frames from the video.
        Spritesheet = "spritesheet",
    }
}

crate::open_string_enum! {
    /// Official `VideoOrder` string domain, with future values preserved.
    pub enum VideoOrder {
        /// Sort results in ascending order.
        Asc = "asc",
        /// Sort results in descending order.
        Desc = "desc",
    }
}

crate::open_string_enum! {
    /// Official `VideoObject` string domain, with future values preserved.
    pub enum VideoObject {
        /// Identifies a video generation job.
        Video = "video",
    }
}

crate::open_string_enum! {
    /// Official `DeletedVideoObject` string domain, with future values preserved.
    pub enum DeletedVideoObject {
        /// Identifies a video deletion confirmation.
        Deleted = "video.deleted",
    }
}

crate::open_string_enum! {
    /// Official `VideoListObject` string domain, with future values preserved.
    pub enum VideoListObject {
        /// Identifies a list of video generation jobs.
        List = "list",
    }
}

/// A video generation job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Video {
    /// Unique identifier of the video generation job.
    pub id: String,
    /// Object type identifying this response as a video job.
    pub object: VideoObject,
    /// Model used to generate the video.
    pub model: VideoModel,
    /// Current state of the generation job.
    pub status: VideoStatus,
    /// Generation progress reported by the service as a percentage.
    pub progress: i64,
    /// Unix timestamp in seconds when the job was created.
    pub created_at: i64,
    /// Unix timestamp in seconds when generation completed, if available.
    pub completed_at: Nullable<i64>,
    /// Unix timestamp in seconds when the generated content expires, if available.
    pub expires_at: Nullable<i64>,
    /// Prompt used to generate the video, if available.
    pub prompt: Nullable<String>,
    /// Dimensions of the generated video.
    pub size: VideoSize,
    /// Duration of the generated video in seconds, encoded as a string.
    pub seconds: String,
    /// Identifier of the source video when this job is a remix.
    pub remixed_from_video_id: Nullable<String>,
    /// Error details when generation fails.
    pub error: Nullable<VideoCreateError>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl Video {
    /// Returns future fields retained from the service.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// An error returned by video generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoCreateError {
    /// Machine-readable error code.
    pub code: String,
    /// Human-readable explanation of the error.
    pub message: String,
    /// Additional alignment information, when returned by the service.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub misalignment: Omittable<ResponseMisalignment>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VideoCreateError {
    /// Returns future fields retained from the service.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A character created from an uploaded video.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoCharacter {
    /// Identifier of the created character, if available.
    pub id: Nullable<String>,
    /// Display name of the character, if available.
    pub name: Nullable<String>,
    /// Unix timestamp in seconds when the character was created.
    pub created_at: i64,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VideoCharacter {
    /// Returns future fields retained from the service.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// Confirmation of video deletion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeletedVideo {
    /// Identifier of the video targeted for deletion.
    pub id: String,
    /// Object type identifying a video deletion response.
    pub object: DeletedVideoObject,
    /// Whether the service deleted the video.
    pub deleted: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl DeletedVideo {
    /// Returns future fields retained from the service.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// A cursor page of video jobs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoList {
    /// Object type identifying a list response.
    pub object: VideoListObject,
    /// Video jobs in this page of results.
    pub data: Vec<Video>,
    /// Identifier of the first video in the page, if present.
    pub first_id: Nullable<String>,
    /// Identifier of the last video in the page, if present.
    pub last_id: Nullable<String>,
    /// Whether another page of results is available.
    pub has_more: bool,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VideoList {
    /// Returns future fields retained from the service.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

/// An image reference; use exactly one of `file_id` and `image_url`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoImageReference {
    /// Identifier of an uploaded image file.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub file_id: Omittable<String>,
    /// URL of the reference image.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub image_url: Omittable<String>,
    #[serde(default, flatten)]
    extra: ExtraFields,
}

impl VideoImageReference {
    /// Returns future fields retained from the service.
    #[must_use]
    pub const fn extra_fields(&self) -> &ExtraFields {
        &self.extra
    }
}

impl VideoImageReference {
    /// Creates a reference to an uploaded image file.
    #[must_use]
    pub fn file_id(id: impl Into<String>) -> Self {
        Self {
            file_id: Omittable::Value(id.into()),
            image_url: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
    /// Creates a reference to an image URL.
    #[must_use]
    pub fn image_url(url: impl Into<String>) -> Self {
        Self {
            image_url: Omittable::Value(url.into()),
            file_id: Omittable::Omitted,
            extra: ExtraFields::new(),
        }
    }
}

/// A video source supplied by upload or existing video id.
#[derive(Clone, Debug, PartialEq)]
pub enum VideoSource {
    /// Upload video content from a replayable multipart source.
    Upload(ReplayableMultipartSource),
    /// Use an existing video by identifier.
    Reference {
        /// Identifier of the existing video.
        id: String,
    },
}

impl VideoSource {
    /// Creates a reference to an existing video.
    #[must_use]
    pub fn id(id: impl Into<String>) -> Self {
        Self::Reference { id: id.into() }
    }
}

/// Optional image guidance for a new video.
#[derive(Clone, Debug, PartialEq)]
pub enum VideoInputReference {
    /// Upload a reference image from a replayable multipart source.
    Upload(ReplayableMultipartSource),
    /// Use an uploaded image file or an image URL as guidance.
    Reference(VideoImageReference),
}

/// New video generation parameters. Omitted values use service defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateVideoRequest {
    /// Text instructions describing the video to generate.
    pub prompt: String,
    /// Model to use; omission selects the service default.
    pub model: Omittable<VideoModel>,
    /// Requested video duration; omission selects the service default.
    pub seconds: Omittable<VideoSeconds>,
    /// Requested video dimensions; omission selects the service default.
    pub size: Omittable<VideoSize>,
    /// Optional reference image to guide generation.
    pub input_reference: Omittable<VideoInputReference>,
}

impl CreateVideoRequest {
    /// Creates a generation request with all optional settings omitted.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: Omittable::Omitted,
            seconds: Omittable::Omitted,
            size: Omittable::Omitted,
            input_reference: Omittable::Omitted,
        }
    }
}

/// Edit an uploaded video or an existing completed video.
#[derive(Clone, Debug, PartialEq)]
pub struct EditVideoRequest {
    /// Uploaded or existing video to edit.
    pub video: VideoSource,
    /// Text instructions describing the requested edits.
    pub prompt: String,
}
impl EditVideoRequest {
    /// Creates an edit request for the supplied video and instructions.
    #[must_use]
    pub fn new(video: VideoSource, prompt: impl Into<String>) -> Self {
        Self {
            video,
            prompt: prompt.into(),
        }
    }
}

/// Extend an uploaded video or an existing completed video.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtendVideoRequest {
    /// Uploaded or existing video to extend.
    pub video: VideoSource,
    /// Text instructions describing the continuation.
    pub prompt: String,
    /// Requested duration of the extension.
    pub seconds: VideoSeconds,
}
impl ExtendVideoRequest {
    /// Creates a request to extend a video by the supplied duration.
    #[must_use]
    pub fn new(video: VideoSource, prompt: impl Into<String>, seconds: VideoSeconds) -> Self {
        Self {
            video,
            prompt: prompt.into(),
            seconds,
        }
    }
}

/// Create a character from a video upload.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateVideoCharacterRequest {
    /// Uploaded video containing the character.
    pub video: ReplayableMultipartSource,
    /// Name to assign to the character.
    pub name: String,
}
impl CreateVideoCharacterRequest {
    /// Creates a character extraction request from an uploaded video.
    #[must_use]
    pub fn new(video: ReplayableMultipartSource, name: impl Into<String>) -> Self {
        Self {
            video,
            name: name.into(),
        }
    }
}

/// New instructions for a video remix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemixVideoRequest {
    /// Text instructions describing the requested remix.
    pub prompt: String,
}
impl RemixVideoRequest {
    /// Creates a remix request with the supplied instructions.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
        }
    }
}

/// Query parameters for the video list endpoint.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListVideosParams {
    /// Cursor identifying the video after which to return results.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub after: Omittable<String>,
    /// Maximum number of video jobs to return.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub limit: Omittable<u32>,
    /// Requested sort order for video jobs.
    #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
    pub order: Omittable<VideoOrder>,
}
