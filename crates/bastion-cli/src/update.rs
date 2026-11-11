//! `bastion update` (F10): replace this install with the newest release,
//! once the release's sums carry the release key's signature and the
//! download matches its sum. A running server goes on with the old build
//! until it is started again.

use std::path::{Path, PathBuf};
use std::process::Command;

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

const REPO: &str = "https://github.com/tacoda/bastion";
/// The release key's public half; `scripts/minisign.pub` holds it.
const PUBKEY: &str = include_str!("../../../scripts/minisign.pub");
/// The version a release build carries; a dev build has none.
const VERSION: Option<&str> = option_env!("BASTION_VERSION");

/// What was installed, read from where this program runs.
#[derive(Debug, PartialEq, Eq)]
enum Install {
    /// `Bastion.app`, at its path.
    App(PathBuf),
    /// A desktop or server tarball unpacked in `lib`, by its folder.
    Tarball { lib: PathBuf, desktop: bool },
}

fn install_of(exe: &Path) -> Option<Install> {
    let app = exe
        .ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"));
    if let Some(app) = app {
        return Some(Install::App(app.to_path_buf()));
    }
    let folder = exe.parent()?.parent()?;
    let name = folder.file_name()?.to_str()?;
    let lib = folder.parent()?.to_path_buf();
    match name {
        n if n.starts_with("bastion-desktop-") => Some(Install::Tarball { lib, desktop: true }),
        n if n.starts_with("bastion-") => Some(Install::Tarball {
            lib,
            desktop: false,
        }),
        _ => None,
    }
}

/// One release, for this machine: its tag and the target it was built for.
struct Release {
    tag: String,
    target: &'static str,
}

impl Release {
    /// The folder it unpacks into: `bastion-<v>` or `bastion-desktop-<v>`.
    fn folder(&self, install: &Install) -> String {
        let v = self.tag.trim_start_matches('v');
        match install {
            Install::Tarball { desktop: false, .. } => format!("bastion-{v}"),
            _ => format!("bastion-desktop-{v}"),
        }
    }

    /// Its file for an install like `install`.
    fn asset(&self, install: &Install) -> String {
        let base = format!("{}-{}", self.folder(install), self.target);
        match install {
            Install::App(_) => format!("{base}.zip"),
            Install::Tarball { .. } => format!("{base}.tar.gz"),
        }
    }

    async fn fetch(&self, file: &str) -> Result<Vec<u8>, String> {
        let url = format!("{REPO}/releases/download/{}/{file}", self.tag);
        let answer = reqwest::get(&url)
            .await
            .map_err(|e| format!("{file}: {e}"))?;
        if !answer.status().is_success() {
            return Err(format!("{file}: {}", answer.status()));
        }
        Ok(answer.bytes().await.map_err(|e| e.to_string())?.to_vec())
    }
}

/// The sum `sums` lists for `file`.
fn sum_of<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|l| {
        let (sum, name) = l.split_once(char::is_whitespace)?;
        (name.trim_start_matches(['*', ' ']) == file).then_some(sum)
    })
}

/// Whether the sums carry the release key's signature.
fn signed(sums: &[u8], signature: &str) -> Result<(), String> {
    let key_line = PUBKEY
        .lines()
        .find(|l| !l.starts_with("untrusted comment"))
        .unwrap_or_default();
    let key = PublicKey::from_base64(key_line.trim())
        .map_err(|_| "this build has no release key, so it cannot check an update".to_string())?;
    let signature =
        Signature::decode(signature).map_err(|e| format!("the release's signature: {e}"))?;
    key.verify(sums, &signature, false)
        .map_err(|_| "the release's signature does not check; nothing was changed".into())
}

pub async fn run() -> Result<(), String> {
    let Some(current) = VERSION else {
        return Err("this is a dev build; update it with git pull and cargo build".into());
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let install =
        install_of(&exe).ok_or_else(|| format!("{} is not an installed Bastion", exe.display()))?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let release = Release {
        tag: newest(&http).await?,
        target: env!("BASTION_TARGET"),
    };
    if release.tag.trim_start_matches('v') == current {
        println!("Bastion {current} is the newest release");
        return Ok(());
    }
    let sums = release.fetch("SHA256SUMS").await?;
    let signature = release.fetch("SHA256SUMS.minisig").await?;
    signed(&sums, &String::from_utf8_lossy(&signature))?;
    let file = release.asset(&install);
    let bytes = release.fetch(&file).await?;
    let sums = String::from_utf8_lossy(&sums);
    let want =
        sum_of(&sums, &file).ok_or_else(|| format!("{file} is not in the release's sums"))?;
    if format!("{:x}", Sha256::digest(&bytes)) != want {
        return Err(format!(
            "{file} does not match its sum; nothing was changed"
        ));
    }
    replace(&install, &release, &bytes)?;
    println!(
        "Bastion {} is installed; start the server again to run it",
        release.tag
    );
    Ok(())
}

/// The newest release's tag, from where `/releases/latest` points.
async fn newest(http: &reqwest::Client) -> Result<String, String> {
    let answer = http
        .get(format!("{REPO}/releases/latest"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let to = answer
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .unwrap_or_default();
    let tag = to.rsplit('/').next().unwrap_or_default();
    match tag.starts_with('v') {
        true => Ok(tag.to_string()),
        false => Err("could not read the newest release".into()),
    }
}

/// Unpack the new release beside the old one, then put it in its place.
fn replace(install: &Install, release: &Release, bytes: &[u8]) -> Result<(), String> {
    let file = release.asset(install);
    let work = std::env::temp_dir().join(format!("bastion-update-{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let archive = work.join(&file);
    std::fs::write(&archive, bytes).map_err(|e| e.to_string())?;
    let run = |program: &str, args: &[&str], at: &Path| -> Result<(), String> {
        let ok = Command::new(program)
            .args(args)
            .current_dir(at)
            .status()
            .map_err(|e| e.to_string())?;
        ok.success()
            .then_some(())
            .ok_or(format!("{program} {}", args.join(" ")))
    };
    let path = archive.display().to_string();
    match install {
        Install::App(app) => {
            run("unzip", &["-q", &path], &work)?;
            let _ = std::fs::remove_dir_all(app);
            std::fs::rename(work.join("Bastion.app"), app).map_err(|e| e.to_string())
        }
        Install::Tarball { lib, .. } => {
            run("tar", &["-xzf", &path], lib)?;
            relink(lib, &release.folder(install))
        }
    }
}

/// Point the links in `bin`, beside `lib`, at the new folder's programs.
fn relink(lib: &Path, folder: &str) -> Result<(), String> {
    let new_bin = lib.join(folder).join("bin");
    let bin = lib.parent().unwrap_or(lib).join("bin");
    for entry in std::fs::read_dir(&new_bin).map_err(|e| format!("{}: {e}", new_bin.display()))? {
        let program = entry.map_err(|e| e.to_string())?.path();
        let link = bin.join(program.file_name().unwrap_or_default());
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&program, &link)
            .map_err(|e| format!("{}: {e}", link.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_install_is_read_from_where_its_binary_is() {
        let app = Path::new("/Users/ada/Applications/Bastion.app/Contents/MacOS/bastion");
        assert_eq!(
            install_of(app),
            Some(Install::App("/Users/ada/Applications/Bastion.app".into()))
        );
        let desk = Path::new("/home/ada/.local/lib/bastion-desktop-0.2.0/bin/bastion");
        let lib = PathBuf::from("/home/ada/.local/lib");
        assert_eq!(
            install_of(desk),
            Some(Install::Tarball {
                lib: lib.clone(),
                desktop: true
            })
        );
        let server = Path::new("/home/ada/.local/lib/bastion-0.2.0/bin/bastion");
        assert_eq!(
            install_of(server),
            Some(Install::Tarball {
                lib,
                desktop: false
            })
        );
        assert_eq!(install_of(Path::new("/repo/target/debug/bastion")), None);
    }

    #[test]
    fn the_release_file_and_its_sum_are_found_by_name() {
        let release = |target| Release {
            tag: "v0.2.0".into(),
            target,
        };
        let server = Install::Tarball {
            lib: "/l".into(),
            desktop: false,
        };
        let linux = release("x86_64-unknown-linux-gnu");
        assert_eq!(linux.folder(&server), "bastion-0.2.0");
        assert_eq!(
            linux.asset(&server),
            "bastion-0.2.0-x86_64-unknown-linux-gnu.tar.gz"
        );
        let app = Install::App("/A".into());
        let mac = release("aarch64-apple-darwin");
        assert_eq!(
            mac.asset(&app),
            "bastion-desktop-0.2.0-aarch64-apple-darwin.zip"
        );
        let sums = "abc  bastion-0.2.0-x.tar.gz\ndef  SHA256SUMS.minisig\n";
        assert_eq!(sum_of(sums, "bastion-0.2.0-x.tar.gz"), Some("abc"));
        assert_eq!(sum_of(sums, "missing"), None);
    }

    #[test]
    fn a_build_without_a_release_key_refuses_to_update() {
        if PUBKEY.contains("REPLACE") {
            assert!(signed(b"sums", "x").unwrap_err().contains("no release key"));
        }
    }
}
