//! Pinned components (ADR-034): every file has a fixed URL (a content-addressed revision),
//! size and SHA-256. Nothing is resolved at run time ("latest"), so what the user consents
//! to is exactly what is installed.

/// One downloadable file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteFile {
    pub url: &'static str,
    pub size: u64,
    /// Lowercase hex SHA-256 of the whole file.
    pub sha256: &'static str,
    /// What to keep from it.
    pub install: Install,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// The file itself, at this path inside the component folder (`/` separators).
    As(&'static str),
    /// Members of a zip archive (a Python wheel): the archive itself is not kept.
    Extract(&'static [Member]),
}

/// One archive member to extract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    /// Path inside the archive.
    pub name: &'static str,
    /// Destination inside the component folder.
    pub dest: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// An installable unit: the embedding model or the inference runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    /// Folder name under the provisioning root (`models/…`, `runtime/…`).
    pub id: &'static str,
    /// Changes whenever any file changes; part of the install path.
    pub version: &'static str,
    /// For the consent text.
    pub title: &'static str,
    pub license: &'static str,
    pub license_url: &'static str,
    /// Host the files come from (consent text, privacy docs).
    pub host: &'static str,
    pub files: &'static [RemoteFile],
    /// Files inside the installed folder holding the license notices to show.
    pub notices: &'static [&'static str],
    /// Only installable where this holds (`cfg!` of the target).
    pub platform_ok: bool,
}

impl Component {
    /// Bytes to download.
    #[must_use]
    pub fn download_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Bytes on disk once installed.
    #[must_use]
    pub fn installed_bytes(&self) -> u64 {
        self.files
            .iter()
            .map(|f| match f.install {
                Install::As(_) => f.size,
                Install::Extract(members) => members.iter().map(|m| m.size).sum(),
            })
            .sum()
    }
}

/// A file of the pinned Hugging Face revision.
macro_rules! hf {
    ($path:literal) => {
        concat!(
            "https://huggingface.co/onnx-community/embeddinggemma-2-ONNX/resolve/",
            "daa72c51243991dfcaf9f9137d2c573d8f7790c0",
            $path
        )
    };
}

/// EmbeddingGemma 2, ONNX export, 4-bit weights (ADR-015), text encoder only.
pub const EMBEDDING_MODEL: Component = Component {
    id: "models/embeddinggemma-2-onnx",
    version: "q4-daa72c5",
    title: "EmbeddingGemma 2 text model (4-bit, ONNX)",
    license: "Apache-2.0",
    license_url: "https://huggingface.co/onnx-community/embeddinggemma-2-ONNX",
    host: "huggingface.co",
    files: &[
        RemoteFile {
            url: hf!("/tokenizer.json"),
            size: 32_170_510,
            sha256: "4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4",
            install: Install::As("tokenizer.json"),
        },
        RemoteFile {
            url: hf!("/onnx/model_q4.onnx"),
            size: 490_742,
            sha256: "f9eeba97acddf139b8ee2ddf04bc30dceafa88de93fadf74d7644e0d61a477a9",
            install: Install::As("onnx/model_q4.onnx"),
        },
        RemoteFile {
            url: hf!("/onnx/model_q4.onnx_data"),
            size: 174_028_800,
            sha256: "c3975f2d1ab7a1878ae31a7d7a9b7804a827aff3800b60dfceafce21cac3df49",
            install: Install::As("onnx/model_q4.onnx_data"),
        },
        RemoteFile {
            url: hf!("/README.md"),
            size: 28_720,
            sha256: "4fe21bf1b86938ac62c7cd2d1511822e4ea6e8654e015e00b06a72676b5420e6",
            install: Install::As("README.md"),
        },
    ],
    notices: &["README.md"],
    platform_ok: true,
};

/// ONNX Runtime 1.30.0 CPU for Windows x64, from the official PyPI wheel (ADR-015/034).
pub const INFERENCE_RUNTIME: Component = Component {
    id: "runtime/onnxruntime-cpu",
    version: "1.30.0-win-x64",
    title: "ONNX Runtime 1.30.0 (CPU)",
    license: "MIT",
    license_url: "https://github.com/microsoft/onnxruntime/blob/main/LICENSE",
    host: "files.pythonhosted.org",
    files: &[RemoteFile {
        url: "https://files.pythonhosted.org/packages/a6/13/0f1699f6de549c9324bc9112a2a85b14c517904cd11b562a654643b755a1/onnxruntime-1.30.0-cp312-cp312-win_amd64.whl",
        size: 14_311_470,
        sha256: "f3501472571f1b1eee50e017851e7929f5ea37312d2d8c2494a19e8fc58b4a38",
        install: Install::Extract(&[
            Member {
                name: "onnxruntime/capi/onnxruntime.dll",
                dest: "onnxruntime.dll",
                size: 18_446_176,
                sha256: "e42c5c917207ff706530315177e39dca2539695a6a8d968a22225e34291b1f12",
            },
            Member {
                name: "onnxruntime/capi/onnxruntime_providers_shared.dll",
                dest: "onnxruntime_providers_shared.dll",
                size: 21_856,
                sha256: "46b999f9d5cd9d284091407e9f50a262624e6a71265fc428a1dfbd42d149bd81",
            },
            Member {
                name: "onnxruntime/LICENSE",
                dest: "LICENSE",
                size: 1_094,
                sha256: "c250d6278f0b47a6439fb7592b08b58a55eb9f535aa49a1db63211c3f982b674",
            },
            Member {
                name: "onnxruntime/ThirdPartyNotices.txt",
                dest: "ThirdPartyNotices.txt",
                size: 344_457,
                sha256: "c53a76501ef60db6f865f20599f220761201ac4057683acefdab37d861b86622",
            },
        ]),
    }],
    notices: &["LICENSE", "ThirdPartyNotices.txt"],
    platform_ok: cfg!(all(windows, target_arch = "x86_64")),
};
