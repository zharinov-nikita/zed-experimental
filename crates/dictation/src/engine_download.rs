//! Engine Download: fetching Engine Assets (a Whisper model or the backend
//! modules) from a fixed catalog into the Zed data directory. The catalog is
//! pinned here, next to the `transcribe-cpp` version in the workspace
//! `Cargo.toml`, so the two are updated in one commit. Downloads go through
//! whatever [`HttpClient`] the caller hands in, so proxy settings apply and
//! tests never touch the network.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, Result, anyhow};
use futures::AsyncReadExt as _;
use http_client::HttpClient;
use sha1::Digest as _;

/// The transcribe.cpp release the backends archive is taken from. Must
/// match the `transcribe-cpp` version in the workspace `Cargo.toml`: the
/// engine only loads backend modules built for its own header hash.
pub const TRANSCRIBE_CPP_VERSION: &str = "0.2.3";

const TRANSCRIBE_CPP_RELEASES: &str =
    "https://github.com/handy-computer/transcribe.cpp/releases/download";
const WHISPER_MODELS_REPOSITORY: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Subdirectories of the dictation data directory.
pub const MODELS_DIR: &str = "models";
pub const BACKENDS_DIR: &str = "backends";

/// A whisper.cpp model from the fixed list. Sizes are the exact byte counts
/// of the files; SHA1 values come from the whisper.cpp models README.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhisperModel {
    /// Name as whisper.cpp uses it in file names (`ggml-<name>.bin`).
    pub name: &'static str,
    pub size_bytes: u64,
    pub sha1: &'static str,
}

impl WhisperModel {
    pub fn file_name(&self) -> String {
        format!("ggml-{}.bin", self.name)
    }

    pub fn url(&self) -> String {
        format!("{WHISPER_MODELS_REPOSITORY}/{}", self.file_name())
    }

    /// Size for humans, e.g. `1.1 GB`.
    pub fn size_label(&self) -> String {
        size_label(self.size_bytes)
    }
}

pub const WHISPER_MODELS: [WhisperModel; 4] = [
    WhisperModel {
        name: "large-v3-q5_0",
        size_bytes: 1_081_140_203,
        sha1: "e6e2ed78495d403bef4b7cff42ef4aaadcfea8de",
    },
    WhisperModel {
        name: "large-v3-turbo-q5_0",
        size_bytes: 574_041_195,
        sha1: "e050f7970618a659205450ad97eb95a18d69c9ee",
    },
    WhisperModel {
        name: "medium-q5_0",
        size_bytes: 539_212_467,
        sha1: "7718d4c1ec62ca96998f058114db98236937490e",
    },
    WhisperModel {
        name: "small-q5_1",
        size_bytes: 190_085_487,
        sha1: "6fe57ddcfdd1c6b07cdcc73aaf620810ce5fc771",
    },
];

/// The backend modules archive for this platform, pinned to
/// [`TRANSCRIBE_CPP_VERSION`]. The SHA256 is the digest GitHub publishes for
/// the release asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendsArchive {
    pub file_name: &'static str,
    /// The folder inside the archive that holds the modules; `backends_dir`
    /// points at it after extraction.
    pub inner_dir: &'static str,
    pub sha256: &'static str,
}

impl BackendsArchive {
    pub fn url(&self) -> String {
        format!(
            "{TRANSCRIBE_CPP_RELEASES}/v{TRANSCRIBE_CPP_VERSION}/{}",
            self.file_name
        )
    }
}

pub const BACKENDS_ARCHIVE: BackendsArchive = BackendsArchive {
    file_name: "transcribe-native-0.2.3-windows-x86_64-cpu-vulkan.tar.gz",
    inner_dir: "transcribe-native-windows-x86_64-cpu-vulkan",
    sha256: "dac5b6038aaf8777cab541b0f854a79e34f13e5892266229f64c61d34a879e49",
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checksum {
    Sha1(String),
    Sha256(String),
}

impl Checksum {
    fn algorithm(&self) -> &'static str {
        match self {
            Checksum::Sha1(_) => "SHA1",
            Checksum::Sha256(_) => "SHA256",
        }
    }

    fn expected(&self) -> &str {
        match self {
            Checksum::Sha1(expected) | Checksum::Sha256(expected) => expected,
        }
    }
}

/// What to do with the downloaded bytes once their checksum is verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// Keep the file under this name in the destination directory.
    File { file_name: String },
    /// Extract the `.tar.gz` into the destination directory; the result is
    /// `inner_dir` inside it.
    TarGz { inner_dir: String },
}

/// One thing to download: where from, what it must hash to, what to make of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetSpec {
    pub url: String,
    pub checksum: Checksum,
    pub kind: AssetKind,
    /// Directory the asset ends up in, relative to the dictation data directory.
    pub subdir: &'static str,
}

impl AssetSpec {
    pub fn model(model: &WhisperModel) -> Self {
        Self {
            url: model.url(),
            checksum: Checksum::Sha1(model.sha1.to_string()),
            kind: AssetKind::File {
                file_name: model.file_name(),
            },
            subdir: MODELS_DIR,
        }
    }

    pub fn backends() -> Self {
        Self {
            url: BACKENDS_ARCHIVE.url(),
            checksum: Checksum::Sha256(BACKENDS_ARCHIVE.sha256.to_string()),
            kind: AssetKind::TarGz {
                inner_dir: BACKENDS_ARCHIVE.inner_dir.to_string(),
            },
            subdir: BACKENDS_DIR,
        }
    }

    /// The path the asset has once the download succeeded.
    pub fn result_path(&self, dictation_dir: &Path) -> PathBuf {
        let dir = dictation_dir.join(self.subdir);
        match &self.kind {
            AssetKind::File { file_name } => dir.join(file_name),
            AssetKind::TarGz { inner_dir } => dir.join(inner_dir),
        }
    }

    fn download_name(&self) -> &str {
        self.url.rsplit('/').next().unwrap_or("download")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    pub received: u64,
    /// From `Content-Length`; `None` when the server did not say.
    pub total: Option<u64>,
}

impl DownloadProgress {
    pub fn percent(&self) -> Option<u8> {
        let total = self.total.filter(|total| *total > 0)?;
        Some((self.received.saturating_mul(100) / total).min(100) as u8)
    }
}

/// The error a download ends with when its cancel flag was raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("download cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Deletes a partial download unless the download completed.
struct PartialFile {
    path: PathBuf,
    keep: bool,
}

impl PartialFile {
    fn complete(&mut self) {
        self.keep = true;
    }
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        if !self.keep && self.path.exists() {
            if let Err(error) = std::fs::remove_file(&self.path) {
                log::warn!(
                    "dictation: could not remove partial download {}: {error}",
                    self.path.display()
                );
            }
        }
    }
}

fn size_label(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}

/// Downloads `spec` under `dictation_dir`, verifying the checksum before
/// anything replaces an existing copy. `progress` is called as bytes arrive;
/// raising `cancel` between chunks ends the download with [`Cancelled`].
/// A partial file, a cancelled download and a checksum mismatch leave
/// nothing behind. Returns [`AssetSpec::result_path`].
pub async fn download_asset(
    client: &dyn HttpClient,
    spec: &AssetSpec,
    dictation_dir: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(DownloadProgress),
) -> Result<PathBuf> {
    let dir = dictation_dir.join(spec.subdir);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut partial = PartialFile {
        path: dir.join(format!("{}.part", spec.download_name())),
        keep: false,
    };

    let mut response = client
        .get(&spec.url, Default::default(), true)
        .await
        .with_context(|| format!("downloading {}", spec.url))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "downloading {} failed with status {}",
            spec.url,
            response.status()
        ));
    }
    let total = response
        .headers()
        .get(http_client::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let mut file = std::fs::File::create(&partial.path)
        .with_context(|| format!("creating {}", partial.path.display()))?;
    let mut sha1 = sha1::Sha1::new();
    let mut sha256 = sha2::Sha256::new();
    let mut received = 0u64;
    let mut chunk = vec![0u8; 256 * 1024];
    let body = response.body_mut();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let read = body
            .read(&mut chunk)
            .await
            .with_context(|| format!("reading {}", spec.url))?;
        if read == 0 {
            break;
        }
        let bytes = &chunk[..read];
        std::io::Write::write_all(&mut file, bytes)
            .with_context(|| format!("writing {}", partial.path.display()))?;
        match spec.checksum {
            Checksum::Sha1(_) => sha1.update(bytes),
            Checksum::Sha256(_) => sha256.update(bytes),
        }
        received += read as u64;
        progress(DownloadProgress { received, total });
    }
    drop(file);

    let actual = match spec.checksum {
        Checksum::Sha1(_) => hex::encode(sha1.finalize()),
        Checksum::Sha256(_) => hex::encode(sha256.finalize()),
    };
    let expected = spec.checksum.expected();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(anyhow!(
            "{} of {} is {actual}, expected {expected}; the file was deleted",
            spec.checksum.algorithm(),
            spec.download_name()
        ));
    }

    let result = spec.result_path(dictation_dir);
    match &spec.kind {
        AssetKind::File { .. } => {
            crate::replace_file(&partial.path, &result)?;
            partial.complete();
        }
        AssetKind::TarGz { .. } => {
            if result.exists() {
                std::fs::remove_dir_all(&result)
                    .with_context(|| format!("replacing {}", result.display()))?;
            }
            let archive = smol::fs::File::open(&partial.path)
                .await
                .with_context(|| format!("opening {}", partial.path.display()))?;
            let decoder = async_compression::futures::bufread::GzipDecoder::new(
                futures::io::BufReader::new(archive),
            );
            async_tar::Archive::new(decoder)
                .unpack(&dir)
                .await
                .with_context(|| format!("extracting {}", spec.download_name()))?;
            if !result.is_dir() {
                return Err(anyhow!(
                    "{} does not contain the folder {}",
                    spec.download_name(),
                    result.display()
                ));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::AsyncWriteExt as _;
    use http_client::{FakeHttpClient, Response};
    use std::cell::RefCell;
    use std::sync::Arc;

    fn client_with_body(
        body: Vec<u8>,
        content_length: bool,
    ) -> Arc<http_client::HttpClientWithUrl> {
        FakeHttpClient::create(move |_| {
            let body = body.clone();
            async move {
                let mut builder = Response::builder().status(200);
                if content_length {
                    builder = builder.header(
                        http_client::http::header::CONTENT_LENGTH,
                        body.len().to_string(),
                    );
                }
                Ok(builder.body(body.into()).unwrap())
            }
        })
    }

    fn sha1_of(bytes: &[u8]) -> String {
        hex::encode(sha1::Sha1::digest(bytes))
    }

    fn sha256_of(bytes: &[u8]) -> String {
        hex::encode(sha2::Sha256::digest(bytes))
    }

    fn model_spec(sha1: String) -> AssetSpec {
        AssetSpec {
            url: "https://test.example/ggml-test.bin".into(),
            checksum: Checksum::Sha1(sha1),
            kind: AssetKind::File {
                file_name: "ggml-test.bin".into(),
            },
            subdir: MODELS_DIR,
        }
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn progress_grows_with_the_body_and_a_correct_checksum_yields_the_path() {
        let body: Vec<u8> = (0..600_000u32).map(|index| (index % 251) as u8).collect();
        let client = client_with_body(body.clone(), true);
        let dir = tempfile::tempdir().unwrap();
        let spec = model_spec(sha1_of(&body));
        let reported = RefCell::new(Vec::<DownloadProgress>::new());

        let result = futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &AtomicBool::new(false),
            |progress| reported.borrow_mut().push(progress),
        ))
        .unwrap();

        assert_eq!(result, dir.path().join(MODELS_DIR).join("ggml-test.bin"));
        assert_eq!(std::fs::read(&result).unwrap(), body);
        let reported = reported.borrow();
        assert!(reported.len() >= 2, "expected several progress reports");
        assert!(
            reported
                .windows(2)
                .all(|pair| pair[0].received < pair[1].received)
        );
        let last = reported.last().unwrap();
        assert_eq!(last.received, body.len() as u64);
        assert_eq!(last.total, Some(body.len() as u64));
        assert_eq!(last.percent(), Some(100));
        assert_eq!(
            names_in(&dir.path().join(MODELS_DIR)),
            vec!["ggml-test.bin"],
            "the partial file must be gone"
        );
    }

    #[test]
    fn a_checksum_mismatch_deletes_the_file_and_reports_an_error() {
        let body = b"not the model".to_vec();
        let client = client_with_body(body, true);
        let dir = tempfile::tempdir().unwrap();
        let spec = model_spec("0000000000000000000000000000000000000000".into());

        let error = futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap_err();

        assert!(error.to_string().contains("SHA1"), "{error}");
        assert!(names_in(&dir.path().join(MODELS_DIR)).is_empty());
    }

    #[test]
    fn cancel_deletes_the_partial_file() {
        let body = vec![7u8; 1_000_000];
        let client = client_with_body(body.clone(), true);
        let dir = tempfile::tempdir().unwrap();
        let spec = model_spec(sha1_of(&body));
        let cancel = AtomicBool::new(false);

        let error = futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &cancel,
            |progress| {
                if progress.received > 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        ))
        .unwrap_err();

        assert!(error.downcast_ref::<Cancelled>().is_some(), "{error}");
        assert!(names_in(&dir.path().join(MODELS_DIR)).is_empty());
    }

    #[test]
    fn a_network_error_is_returned_as_an_error() {
        let client = FakeHttpClient::create(|_| async move { Err(anyhow!("connection refused")) });
        let dir = tempfile::tempdir().unwrap();
        let spec = model_spec(sha1_of(b"x"));

        let error = futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("connection refused"),
            "{error:#}"
        );

        let client = FakeHttpClient::with_404_response();
        let error = futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap_err();
        assert!(error.to_string().contains("404"), "{error}");
        assert!(names_in(&dir.path().join(MODELS_DIR)).is_empty());
    }

    #[test]
    fn progress_without_content_length_has_no_percent() {
        let body = vec![1u8; 10_000];
        let client = client_with_body(body.clone(), false);
        let dir = tempfile::tempdir().unwrap();
        let spec = model_spec(sha1_of(&body));
        let last = RefCell::new(None);

        futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &AtomicBool::new(false),
            |progress| *last.borrow_mut() = Some(progress),
        ))
        .unwrap();

        let last = last.borrow().unwrap();
        assert_eq!(last.total, None);
        assert_eq!(last.percent(), None);
    }

    async fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut gzip = async_compression::futures::write::GzipEncoder::new(Vec::new());
        {
            let mut builder = async_tar::Builder::new(&mut gzip);
            for (name, data) in entries {
                let mut header = async_tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, name, *data).await.unwrap();
            }
            builder.finish().await.unwrap();
        }
        gzip.close().await.unwrap();
        gzip.into_inner()
    }

    #[test]
    fn the_backends_archive_is_extracted_and_the_inner_folder_returned() {
        let archive = futures::executor::block_on(tar_gz(&[
            ("transcribe-native-test/transcribe.dll", b"dll"),
            ("transcribe-native-test/ggml-vulkan.dll", b"vulkan"),
            ("transcribe-native-test/licenses/LICENSE", b"gpl"),
        ]));
        let client = client_with_body(archive.clone(), true);
        let dir = tempfile::tempdir().unwrap();
        let spec = AssetSpec {
            url: "https://test.example/transcribe-native-test.tar.gz".into(),
            checksum: Checksum::Sha256(sha256_of(&archive)),
            kind: AssetKind::TarGz {
                inner_dir: "transcribe-native-test".into(),
            },
            subdir: BACKENDS_DIR,
        };

        let result = futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap();

        assert_eq!(
            result,
            dir.path().join(BACKENDS_DIR).join("transcribe-native-test")
        );
        assert_eq!(
            std::fs::read(result.join("ggml-vulkan.dll")).unwrap(),
            b"vulkan"
        );
        assert_eq!(
            std::fs::read(result.join("licenses").join("LICENSE")).unwrap(),
            b"gpl"
        );
        assert_eq!(
            names_in(&dir.path().join(BACKENDS_DIR)),
            vec!["transcribe-native-test"],
            "the archive must be removed after extraction"
        );
    }

    #[test]
    fn a_wrong_archive_checksum_deletes_the_archive() {
        let archive = futures::executor::block_on(tar_gz(&[("x/transcribe.dll", b"dll")]));
        let client = client_with_body(archive, true);
        let dir = tempfile::tempdir().unwrap();
        let spec = AssetSpec {
            url: "https://test.example/transcribe-native-test.tar.gz".into(),
            checksum: Checksum::Sha256("00".repeat(32)),
            kind: AssetKind::TarGz {
                inner_dir: "x".into(),
            },
            subdir: BACKENDS_DIR,
        };

        let error = futures::executor::block_on(download_asset(
            client.as_ref(),
            &spec,
            dir.path(),
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap_err();

        assert!(error.to_string().contains("SHA256"), "{error}");
        assert!(names_in(&dir.path().join(BACKENDS_DIR)).is_empty());
    }

    #[test]
    fn the_catalog_is_pinned_to_the_linked_engine_version() {
        let workspace_manifest = include_str!("../../../Cargo.toml");
        let pinned = format!("transcribe-cpp = {{ version = \"{TRANSCRIBE_CPP_VERSION}\"");
        assert!(
            workspace_manifest.contains(&pinned),
            "TRANSCRIBE_CPP_VERSION must match the transcribe-cpp dependency in Cargo.toml"
        );
        assert!(BACKENDS_ARCHIVE.file_name.contains(TRANSCRIBE_CPP_VERSION));
        assert_eq!(BACKENDS_ARCHIVE.sha256.len(), 64);
        for model in &WHISPER_MODELS {
            assert_eq!(model.sha1.len(), 40, "{}", model.name);
            assert!(model.url().ends_with(&format!("ggml-{}.bin", model.name)));
        }
        assert_eq!(WHISPER_MODELS[0].size_label(), "1.1 GB");
        assert_eq!(WHISPER_MODELS[3].size_label(), "190 MB");
    }

    #[test]
    fn result_paths_live_under_the_dictation_directory() {
        let root = Path::new("data").join("dictation");
        assert_eq!(
            AssetSpec::model(&WHISPER_MODELS[3]).result_path(&root),
            root.join("models").join("ggml-small-q5_1.bin")
        );
        assert_eq!(
            AssetSpec::backends().result_path(&root),
            root.join("backends").join(BACKENDS_ARCHIVE.inner_dir)
        );
    }
}
