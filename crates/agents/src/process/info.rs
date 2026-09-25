/// Executable and launch entrypoints, without OS handles or session metadata.
#[derive(Default)]
pub struct ProcessInfo<'a> {
    pub executable: Option<&'a str>,
    pub command: Option<&'a str>,
    pub script: Option<&'a str>,
}

impl<'a> ProcessInfo<'a> {
    pub fn new(executable: Option<&'a str>, arguments: &[&'a str]) -> Self {
        let executable = executable.map(normalize_path);
        let command = arguments.first().copied().map(normalize_path);
        Self {
            executable,
            command,
            script: interpreter_script(executable.or(command), arguments).map(normalize_path),
        }
    }

    pub fn entrypoints(&self) -> impl Iterator<Item = &'a str> {
        [self.executable, self.command, self.script]
            .into_iter()
            .flatten()
    }

    pub fn matches_command(&self, commands: &[&str]) -> bool {
        self.entrypoints()
            .any(|path| commands.contains(&basename(path)))
    }

    pub(super) fn candidates(&self) -> [Self; 3] {
        // The executable takes precedence over argv[0], then the interpreter script.
        [
            Self {
                executable: self.executable,
                ..Self::default()
            },
            Self {
                command: self.command,
                ..Self::default()
            },
            Self {
                script: self.script,
                ..Self::default()
            },
        ]
    }
}

fn normalize_path(path: &str) -> &str {
    path.strip_suffix(" (deleted)").unwrap_or(path)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn interpreter_script<'a>(executable: Option<&str>, arguments: &[&'a str]) -> Option<&'a str> {
    let interpreter = basename(executable?);
    let python = matches!(interpreter, "python" | "Python")
        || interpreter.strip_prefix("python").is_some_and(|version| {
            !version.is_empty()
                && version
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        });
    if !python && !matches!(interpreter, "node" | "nodejs" | "bun") {
        return None;
    }
    let mut arguments = arguments.iter().skip(1);
    while let Some(argument) = arguments.next() {
        match *argument {
            "-e" | "--eval" | "-p" | "--print" | "-c" | "-m" => return None,
            option if option.starts_with("--eval=") || option.starts_with("--print=") => {
                return None;
            }
            "-r" | "--require" | "--import" | "--loader" => {
                arguments.next();
            }
            "--" => {}
            option if option.starts_with('-') => {}
            entrypoint => return Some(entrypoint),
        }
    }
    None
}
