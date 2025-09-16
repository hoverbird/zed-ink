use std::fs;
use std::process::Command;
use zed_extension_api::{self as zed, LanguageServerId, Result, settings::LspSettings};

struct InkLanguageServerBinary {
    path: String,
    args: Option<Vec<String>>,
}

struct InkExtension {
    lsp_binary_path: Option<String>, // Track the LSP-capable binary separately
    verified_system_lsp: Option<bool>, // Cache whether system binary supports LSP
}

impl InkExtension {
    fn test_lsp_support(&self, binary_path: &str) -> bool {
        // Test if the binary supports LSP by checking its help output
        match Command::new(binary_path).arg("--help").output() {
            Ok(output) => {
                let help_text = String::from_utf8_lossy(&output.stdout);
                // Look for LSP-related flags in help output
                help_text.contains("--language-server")
                    || help_text.contains("language server")
                    || help_text.contains("LSP")
            }
            Err(_) => false,
        }
    }

    fn language_server_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<InkLanguageServerBinary> {
        let binary_settings = LspSettings::for_worktree("ink-lsp", worktree)
            .ok()
            .and_then(|lsp_settings| lsp_settings.binary);
        let binary_args = binary_settings
            .as_ref()
            .and_then(|binary_settings| binary_settings.arguments.clone());

        // 1. Check user-configured binary (highest priority)
        if let Some(path) = binary_settings.and_then(|binary_settings| binary_settings.path) {
            return Ok(InkLanguageServerBinary {
                path,
                args: binary_args,
            });
        }

        // 2. Check if we already have a verified LSP binary cached
        if let Some(path) = &self.lsp_binary_path {
            if fs::metadata(path).map_or(false, |stat| stat.is_file()) {
                return Ok(InkLanguageServerBinary {
                    path: path.clone(),
                    args: binary_args,
                });
            } else {
                // Cached binary no longer exists, clear cache
                self.lsp_binary_path = None;
            }
        }

        // 3. Check system inklecate and test if it supports LSP
        if let Some(system_path) = worktree.which("inklecate") {
            match self.verified_system_lsp {
                Some(true) => {
                    // We know this system binary supports LSP
                    return Ok(InkLanguageServerBinary {
                        path: system_path,
                        args: binary_args,
                    });
                }
                Some(false) => {
                    // We know system binary doesn't support LSP, skip to download
                    zed::set_language_server_installation_status(
                        language_server_id,
                        &zed::LanguageServerInstallationStatus::CheckingForUpdate,
                    );
                }
                None => {
                    // Test if system binary supports LSP
                    if self.test_lsp_support(&system_path) {
                        self.verified_system_lsp = Some(true);
                        return Ok(InkLanguageServerBinary {
                            path: system_path,
                            args: binary_args,
                        });
                    } else {
                        self.verified_system_lsp = Some(false);
                        zed::set_language_server_installation_status(
                            language_server_id,
                            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
                        );
                        // Continue to download LSP version
                    }
                }
            }
        }

        // 4. Download LSP-capable version
        let release = zed::latest_github_release(
            "yuna0x0/ink-lsp",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (platform, arch) = zed::current_platform();
        let platform_name = match platform {
            zed::Os::Mac => "osx",
            zed::Os::Linux => "linux",
            zed::Os::Windows => "win",
        };
        let arch_name = match arch {
            zed::Architecture::Aarch64 => "arm64",
            zed::Architecture::X86 => "x86",
            zed::Architecture::X8664 => "x64",
        };

        let asset_name = format!("inklecate-{}-{}.zip", platform_name, arch_name);

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "No LSP-capable inklecate found for platform {}-{}. Available assets: {:?}",
                    platform_name,
                    arch_name,
                    release.assets.iter().map(|a| &a.name).collect::<Vec<_>>()
                )
            })?;

        // Use unique naming to avoid conflicts with system inklecate
        let version_dir = format!("inklecate-lsp-{}", release.version);
        let binary_name = match platform {
            zed::Os::Windows => "inklecate-lsp.exe",
            _ => "inklecate-lsp",
        };
        let binary_path = format!("{}/{}", version_dir, binary_name);

        if !fs::metadata(&binary_path).map_or(false, |stat| stat.is_file()) {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &version_dir,
                zed::DownloadedFileType::Zip,
            )
            .map_err(|e| format!("Failed to download LSP-capable inklecate: {}", e))?;

            // Extract and rename the binary to avoid confusion with system inklecate
            let extracted_binary = format!(
                "{}/inklecate{}",
                version_dir,
                if platform == zed::Os::Windows {
                    ".exe"
                } else {
                    ""
                }
            );

            if fs::metadata(&extracted_binary).is_ok() {
                fs::rename(&extracted_binary, &binary_path)
                    .map_err(|e| format!("Failed to rename downloaded binary: {}", e))?;
            }

            // Clean up old versions
            let entries = fs::read_dir(".")
                .map_err(|e| format!("Failed to list working directory: {}", e))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("Failed to load directory entry: {}", e))?;
                let name = entry.file_name();
                let name_str = name.to_str().unwrap_or("");
                if name_str.starts_with("inklecate-lsp-") && name_str != version_dir {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }

            zed::make_file_executable(&binary_path)?;
        }

        // Cache the LSP binary path for future use
        self.lsp_binary_path = Some(binary_path.clone());

        Ok(InkLanguageServerBinary {
            path: binary_path,
            args: binary_args,
        })
    }
}

impl zed::Extension for InkExtension {
    fn new() -> Self {
        Self {
            lsp_binary_path: None,
            verified_system_lsp: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let inklecate_binary = self.language_server_binary(language_server_id, worktree)?;

        // Use --language-server flag for LSP mode (not -l which doesn't exist)
        let args = inklecate_binary
            .args
            .unwrap_or_else(|| vec!["--language-server".into()]);

        Ok(zed::Command {
            command: inklecate_binary.path,
            args,
            env: Default::default(),
        })
    }
}

zed::register_extension!(InkExtension);
