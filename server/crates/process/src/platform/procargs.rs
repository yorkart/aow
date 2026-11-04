//! Decode Darwin KERN_PROCARGS2 without flattening argv into shell text.

use std::io;

pub(super) fn parse_procargs(bytes: &[u8]) -> io::Result<(&[u8], Option<&[u8]>)> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid process arguments");
    let header = bytes.get(..4).ok_or_else(invalid)?;
    let argc = i32::from_ne_bytes(header.try_into().unwrap());
    if argc <= 0 || argc as usize > bytes.len() {
        return Err(invalid());
    }
    // Saved executable path, followed by NUL alignment before argv[0].
    let tail = &bytes[4..];
    let path_end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(invalid)?;
    let argv_start = 4
        + path_end
        + tail[path_end..]
            .iter()
            .position(|byte| *byte != 0)
            .ok_or_else(invalid)?;
    let mut end = argv_start;
    for _ in 0..argc {
        end += bytes[end..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(invalid)?
            + 1;
    }
    let arguments = &bytes[argv_start..end];
    // For protected executables Darwin successfully returns only argv, ending
    // exactly here. This is not evidence of an empty environment: falling back
    // to the server's HOME/config in that case could bind the wrong session.
    if end == bytes.len() {
        return Ok((arguments, None));
    }
    let env_start = end;
    while end < bytes.len() && bytes[end] != 0 {
        end += bytes[end..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(invalid)?
            + 1;
    }
    Ok((arguments, Some(&bytes[env_start..end])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(argv: &[&[u8]], environment: &[&[u8]]) -> Vec<u8> {
        let mut bytes = (argv.len() as i32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/opt/node with spaces\0\0\0");
        for value in argv.iter().chain(environment) {
            bytes.extend_from_slice(value);
            bytes.push(0);
        }
        bytes.extend_from_slice(b"\0\0");
        bytes
    }

    #[test]
    fn preserves_empty_arguments_spaces_and_environment_boundaries() {
        let bytes = snapshot(
            &[
                b"node",
                b"/opt/claude code/cli.js",
                b"",
                b"HOME=/not-an-env",
                b"prompt with spaces",
            ],
            &[
                b"HOME=/home/user name",
                b"CLAUDE_CONFIG_DIR=/custom config",
                b"RAW=\xff",
            ],
        );
        let (argv, env) = parse_procargs(&bytes).unwrap();
        assert_eq!(
            argv,
            b"node\0/opt/claude code/cli.js\0\0HOME=/not-an-env\0prompt with spaces\0"
        );
        assert_eq!(
            env.unwrap(),
            b"HOME=/home/user name\0CLAUDE_CONFIG_DIR=/custom config\0RAW=\xff\0"
        );
    }

    #[test]
    fn empty_environment_stops_before_trailing_kernel_data() {
        let mut bytes = snapshot(&[b"claude"], &[]);
        bytes.extend_from_slice(b"HOME=/not-the-environment\0");
        assert_eq!(
            parse_procargs(&bytes).unwrap(),
            (&b"claude\0"[..], Some(&b""[..]))
        );
    }

    #[test]
    fn redacted_environment_is_distinct_from_an_empty_environment() {
        let bytes = [1i32.to_ne_bytes().as_slice(), b"/bin/cat\0\0cat\0"].concat();
        assert_eq!(parse_procargs(&bytes).unwrap(), (&b"cat\0"[..], None));
    }

    #[test]
    fn rejects_truncated_or_invalid_snapshots() {
        for bytes in [
            vec![],
            vec![1, 0, 0],
            (-1i32).to_ne_bytes().to_vec(),
            0i32.to_ne_bytes().to_vec(),
            [1i32.to_ne_bytes().as_slice(), b"unterminated path"].concat(),
            [
                2i32.to_ne_bytes().as_slice(),
                b"/bin/node\0node\0missing argument",
            ]
            .concat(),
            [
                1i32.to_ne_bytes().as_slice(),
                b"/bin/node\0node\0HOME=truncated",
            ]
            .concat(),
        ] {
            assert!(parse_procargs(&bytes).is_err());
        }
    }
}
