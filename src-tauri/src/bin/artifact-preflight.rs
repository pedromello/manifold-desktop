use manifold_desktop_lib::publisher::{preflight_declaration, ManifestDeclaration};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

#[derive(Debug)]
struct Options {
    archive: PathBuf,
    entrypoint: String,
    working_directory: Option<String>,
    launch_arguments: Vec<String>,
    executables: Vec<String>,
    environment: BTreeMap<String, String>,
    output: Option<PathBuf>,
}

fn main() {
    if let Err(error) = run(env::args().skip(1)) {
        eprintln!("artifact preflight failed: {}", error);
        std::process::exit(1);
    }
}

fn run(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let options = parse_options(arguments)?;
    if options.output.as_ref() == Some(&options.archive) {
        return Err("output path must not overwrite the ZIP archive".into());
    }
    let output = options.output.clone();
    let declaration = preflight_declaration(
        &options.archive,
        ManifestDeclaration {
            schema_version: "1".into(),
            entrypoint: options.entrypoint,
            launch_arguments: options.launch_arguments,
            working_directory: options.working_directory,
            executables: options.executables,
            environment: options.environment,
        },
    )
    .map_err(|error| error.message)?;
    let json = serde_json::to_string_pretty(&declaration)
        .map_err(|error| format!("could not serialize upload declaration: {error}"))?;
    if let Some(path) = output {
        fs::write(&path, format!("{json}\n"))
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        eprintln!("artifact preflight passed: {}", path.display());
    } else {
        println!("{json}");
    }
    Ok(())
}

fn parse_options(arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut arguments = arguments.peekable();
    let archive = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let mut entrypoint = None;
    let mut working_directory = None;
    let mut launch_arguments = Vec::new();
    let mut executables = Vec::new();
    let mut environment = BTreeMap::new();
    let mut output = None;

    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} requires a value\n{}", usage()))?;
        match flag.as_str() {
            "--entrypoint" => entrypoint = Some(value),
            "--working-directory" => working_directory = Some(value),
            "--launch-argument" => launch_arguments.push(value),
            "--executable" => executables.push(value),
            "--environment" => {
                let (key, value) = value
                    .split_once('=')
                    .filter(|(key, _)| !key.is_empty())
                    .ok_or("--environment must use KEY=VALUE")?;
                environment.insert(key.to_string(), value.to_string());
            }
            "--output" => output = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown option {flag}\n{}", usage())),
        }
    }

    let entrypoint = entrypoint.ok_or_else(usage)?;
    Ok(Options {
        archive,
        entrypoint,
        working_directory,
        launch_arguments,
        executables,
        environment,
        output,
    })
}

fn usage() -> String {
    "usage: artifact-preflight <archive.zip> --entrypoint <relative-path> [--working-directory <relative-path>] [--launch-argument <value>] [--executable <relative-path>] [--environment KEY=VALUE] [--output <declaration.json>]".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn requires_an_archive_and_entrypoint() {
        assert!(parse_options(Vec::<String>::new().into_iter()).is_err());
        assert!(parse_options(vec!["game.zip".into()].into_iter()).is_err());
    }

    #[test]
    fn parses_manifest_options_without_interpreting_values() {
        let options = parse_options(
            vec![
                "game.zip".into(),
                "--entrypoint".into(),
                "bin\\game.exe".into(),
                "--launch-argument".into(),
                "--safe-mode".into(),
                "--environment".into(),
                "CHANNEL=preview".into(),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(options.archive, Path::new("game.zip"));
        assert_eq!(options.entrypoint, "bin\\game.exe");
        assert_eq!(options.launch_arguments, vec!["--safe-mode"]);
        assert_eq!(
            options.environment.get("CHANNEL").map(String::as_str),
            Some("preview")
        );
    }
}
