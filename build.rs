//! The commit a build is made from, for `quillo --version`: `QUILLO_COMMIT`
//! when whoever builds sets it, else what git says of this checkout ("hash,
//! date"); empty when neither knows (a source archive).

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn main() {
    let commit = std::env::var("QUILLO_COMMIT").ok().filter(|c| !c.is_empty()).or_else(|| {
        let hash = git(&["rev-parse", "--short=7", "HEAD"])?;
        Some(match git(&["show", "-s", "--format=%cs", "HEAD"]) {
            Some(date) => format!("{}, {}", hash, date),
            None => hash,
        })
    });
    println!("cargo:rustc-env=QUILLO_COMMIT={}", commit.unwrap_or_default());
    println!("cargo:rerun-if-env-changed=QUILLO_COMMIT");
    // Built again when HEAD moves: a commit, a checkout, a pull.
    if let (Some(dir), Some(common)) = (git(&["rev-parse", "--git-dir"]), git(&["rev-parse", "--git-common-dir"])) {
        println!("cargo:rerun-if-changed={}/HEAD", dir);
        if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
            let file = format!("{}/{}", common, branch);
            if std::path::Path::new(&file).exists() {
                println!("cargo:rerun-if-changed={}", file);
            }
        }
        let packed = format!("{}/packed-refs", common);
        if std::path::Path::new(&packed).exists() {
            println!("cargo:rerun-if-changed={}", packed);
        }
    }
}
