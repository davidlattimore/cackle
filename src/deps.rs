//! Locates and parses depinfo emitted by the rust compiler.

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use std::path::PathBuf;

/// Uses the supplied rustc arguments to determine where the deps file will be located, then reads
/// it and extracts the paths of all the source files.
pub(crate) fn source_files_from_rustc_args(
    args: impl Iterator<Item = String>,
) -> Result<Vec<PathBuf>> {
    let Some(deps_path) = deps_path_from_rustc_args(args)? else {
        return Ok(vec![]);
    };
    let deps = std::fs::read_to_string(&deps_path)
        .with_context(|| format!("Failed to read deps file `{}`", deps_path.display()))?;
    Ok(parse_deps(&deps)?
        .into_iter()
        .flat_map(|dep| dep.canonicalize())
        .collect())
}

fn parse_deps(deps_text: &str) -> Result<Vec<PathBuf>> {
    let mut deps = Vec::new();
    for line in deps_text.lines() {
        if let Some(filename) = line.strip_suffix(':') {
            deps.push(PathBuf::from(filename));
        }
    }
    Ok(deps)
}

fn deps_path_from_rustc_args(mut args: impl Iterator<Item = String>) -> Result<Option<PathBuf>> {
    let mut crate_name = None;
    let mut extra = String::new();
    let mut out_dir = None;
    let mut emit_dep_info = false;
    while let Some(arg) = args.next() {
        if arg == "-C" {
            let Some(arg) = args.next() else {
                bail!("Missing argument to -C");
            };
            if let Some(rest) = arg.strip_prefix("extra-filename=") {
                extra = rest.to_owned();
            }
        } else if arg == "--out-dir" {
            let Some(arg) = args.next() else {
                bail!("Missing argument to --out-dir");
            };
            out_dir = Some(arg);
        } else if arg == "--crate-name" {
            let Some(arg) = args.next() else {
                bail!("Missing argument to --crate-name");
            };
            crate_name = Some(arg);
        } else if arg.starts_with("--emit=") {
            emit_dep_info = arg.contains("dep-info");
        }
    }
    if !emit_dep_info {
        return Ok(None);
    }
    let crate_name = crate_name.ok_or_else(|| anyhow!("Missing --crate-name"))?;
    let out_dir = out_dir.ok_or_else(|| anyhow!("Missing --out-dir"))?;
    Ok(Some(
        Path::new(&out_dir).join(format!("{crate_name}{extra}.d")),
    ))
}

#[cfg(test)]
mod tests {
    use super::deps_path_from_rustc_args;
    use super::parse_deps;
    use anyhow::Result;
    use std::path::PathBuf;

    fn deps_path(args: &[&str]) -> Result<Option<PathBuf>> {
        deps_path_from_rustc_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn test_source_files_from_rustc_args() {
        let deps_path = deps_path(&[
            "rustc",
            "--emit=dep-info,link",
            "--crate-name",
            "foo",
            "-C",
            "extra-filename=-0188200cb614ae3d",
            "--out-dir",
            "/some/directory/target/debug/deps",
        ])
        .unwrap();
        assert_eq!(
            deps_path,
            Some(PathBuf::from(
                "/some/directory/target/debug/deps/foo-0188200cb614ae3d.d"
            ))
        );
    }

    #[test]
    fn test_source_files_from_rustc_args_missing_crate_name() {
        assert!(deps_path(&[
            "rustc",
            "--emit=dep-info,link",
            "-C",
            "extra-filename=-0188200cb614ae3d",
            "--out-dir",
            "/some/directory/target/debug/deps",
        ])
        .is_err());
    }

    #[test]
    fn test_source_files_from_rustc_args_missing_out_dir() {
        assert!(deps_path(&[
            "rustc",
            "--emit=dep-info,link",
            "--crate-name",
            "foo",
            "-C",
            "extra-filename=-0188200cb614ae3d",
        ])
        .is_err());
    }

    #[test]
    fn test_source_files_from_rustc_args_no_dep_info() {
        assert_eq!(deps_path(&[]).unwrap(), None);
    }

    fn path_strings(input: &[PathBuf]) -> Vec<&str> {
        input.iter().filter_map(|path| path.to_str()).collect()
    }

    #[test]
    fn test_parse_deps() {
        let deps = parse_deps(indoc::indoc! {r#"
            /some/path/foo-1235.rmeta: foo/src/lib.rs /some/absolute/path/extra.rs

            /some/path/foo-1235.rlib: foo/src/lib.rs /some/absolute/path/extra.rs

            foo/src/lib.rs:
            /some/absolute/path/extra.rs:

            # env-dep:OUT_DIR=/some/path/target/debug/build/foo-1235/out
            "#})
        .unwrap();
        assert_eq!(
            path_strings(&deps),
            &["foo/src/lib.rs", "/some/absolute/path/extra.rs"]
        )
    }
}
