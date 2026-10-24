//! Starting the app. The profile goes to the app twice: as `--profile NAME` and as the variable
//! `KADAKAS_PROFILE`. The app reads either.

use crate::error::{Result, UpdateError};
use crate::flavors::Flavor;
use crate::settings::valid_name;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// The command that starts `flavor` from the version folder `dir`.
/// An empty profile means no profile. A bad profile name is an error.
pub fn command(dir: &Path, flavor: &Flavor, profile: &str) -> Result<Command> {
    command_with(dir, flavor, profile, &[], &[])
}

/// Like [`command`], with more arguments (for example the files to open) and more variables
/// (the run id and the install folder, see `handshake`).
pub fn command_with(
    dir: &Path,
    flavor: &Flavor,
    profile: &str,
    args: &[OsString],
    env: &[(String, String)],
) -> Result<Command> {
    let mut cmd = Command::new(dir.join(&flavor.exe));
    cmd.current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if profile.is_empty() {
        cmd.env_remove("KADAKAS_PROFILE");
    } else if valid_name(profile) {
        cmd.arg("--profile").arg(profile);
        cmd.env("KADAKAS_PROFILE", profile);
    } else {
        return Err(UpdateError::Manifest(format!(
            "bad profile name {profile:?}"
        )));
    }
    // The files come after the options, and after `--`, so a file name that starts with `-`
    // is not read as an option.
    if !args.is_empty() {
        cmd.arg("--").args(args);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    Ok(cmd)
}

/// The command that starts the main app (`kadakas-app`) with `files`, from another program of the
/// same version folder (the 3D modeler's "Open in the Kadakas app"). `exe` is the path of the
/// running program: the main app is next to it. The new app is not run by the launcher: it has no
/// run record, so no crash mark and no restart for an update, and `KADAKAS_VIA_LAUNCHER` is set so
/// that on macOS it does not hand over to the launcher (which would start the flavor of the
/// person's choice, maybe the modeler again). `None` if the main app is not next to `exe`.
pub fn sibling_app_command(exe: &Path, files: &[OsString]) -> Option<Command> {
    let dir = exe.parent()?;
    let rel = crate::flavors::find_exe(dir, crate::settings::MAIN_FLAVOR)?;
    let flavor = Flavor {
        id: crate::settings::MAIN_FLAVOR.into(),
        name: "Kadakas".into(),
        exe: rel,
    };
    let env = [(crate::handshake::ENV_VIA.to_string(), "1".to_string())];
    let mut cmd = command_with(dir, &flavor, "", files, &env).ok()?;
    // The run of the sender is not the run of the new app.
    cmd.env_remove(crate::handshake::ENV_RUN)
        .env_remove(crate::handshake::ENV_ROOT);
    Some(cmd)
}

/// The program `program` of the flavors (`kadakas-modeler`) next to the running program `exe`, if the
/// installation has it.
pub fn sibling_exists(exe: &Path, program: &str) -> bool {
    exe.parent()
        .is_some_and(|dir| crate::flavors::find_exe(dir, program).is_some())
}

/// The command that starts the program `program` next to the running program `exe` with `files`
/// (the "Open in modeler" of the app, MOD-13). The run of the sender is not the run of the new
/// program. `None` if the program is not next to `exe`.
pub fn sibling_command(exe: &Path, program: &str, files: &[OsString]) -> Option<Command> {
    let dir = exe.parent()?;
    let rel = crate::flavors::find_exe(dir, program)?;
    let flavor = Flavor {
        id: program.into(),
        name: program.into(),
        exe: rel,
    };
    let mut cmd = command_with(dir, &flavor, "", files, &[]).ok()?;
    cmd.env_remove(crate::handshake::ENV_RUN)
        .env_remove(crate::handshake::ENV_ROOT);
    Some(cmd)
}

/// How a file went to a sibling program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// A new process was started.
    Spawned,
    /// The program runs already and was asked to open the file.
    Asked,
}

/// Why a file did not go to a sibling program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SiblingError {
    /// The program is not installed next to this one.
    Missing,
    /// The program did not start.
    Failed(String),
}

/// Opens `file` in the program `program` next to `exe`. If that program runs (it has an alive stamp
/// that names it, see `handshake::running_program`), the file goes to it as an open request and no
/// second process starts. Otherwise the program is started with the file. `root` is the folder of
/// the launcher and `own` the id of this run.
pub fn open_in_sibling(
    root: Option<&Path>,
    own: &str,
    exe: &Path,
    program: &str,
    file: &Path,
) -> std::result::Result<Sent, SiblingError> {
    if !sibling_exists(exe, program) {
        return Err(SiblingError::Missing);
    }
    if let Some(root) = root
        && let Some(id) = crate::handshake::running_program(root, program, own)
        && crate::handshake::send_open(root, &id, &[file.to_path_buf()]).is_ok()
    {
        return Ok(Sent::Asked);
    }
    let mut cmd = sibling_command(exe, program, &[file.as_os_str().to_owned()])
        .ok_or(SiblingError::Missing)?;
    let mut child = cmd
        .spawn()
        .map_err(|e| SiblingError::Failed(e.to_string()))?;
    // Collect the child when it ends, so that it does not stay a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(Sent::Spawned)
}

/// Starts the app and does not wait for it.
pub fn launch(dir: &Path, flavor: &Flavor, profile: &str) -> Result<Child> {
    Ok(command(dir, flavor, profile)?.spawn()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::store::tests::tmp;
    use std::ffi::OsStr;

    fn flavor() -> Flavor {
        Flavor {
            id: "kadakas-app".into(),
            name: "Kadakas".into(),
            exe: "kadakas-app".into(),
        }
    }

    #[test]
    fn profile_is_an_argument_and_a_variable() {
        let c = command(Path::new("/v/1.0.0"), &flavor(), "acad").unwrap();
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, ["--profile", "acad"]);
        assert_eq!(
            c.get_program(),
            Path::new("/v/1.0.0").join("kadakas-app").as_os_str()
        );
        let env: Vec<_> = c.get_envs().collect();
        assert_eq!(
            env,
            [(OsStr::new("KADAKAS_PROFILE"), Some(OsStr::new("acad")))]
        );
    }

    #[test]
    fn files_and_variables_are_passed_on() {
        let files = [OsString::from("/a/b.kad"), OsString::from("-odd.kad")];
        let env = [("KADAKAS_RUN_ID".to_string(), "1a".to_string())];
        let c = command_with(Path::new("/v"), &flavor(), "p", &files, &env).unwrap();
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, ["--profile", "p", "--", "/a/b.kad", "-odd.kad"]);
        let envs: Vec<_> = c.get_envs().collect();
        assert!(envs.contains(&(OsStr::new("KADAKAS_RUN_ID"), Some(OsStr::new("1a")))));
    }

    #[test]
    fn a_bundle_is_started_by_its_main_executable() {
        let bundle = Flavor {
            exe: crate::flavors::bundle_exe("kadakas-app"),
            ..flavor()
        };
        let files = [OsString::from("/a/b.kad")];
        let env = [("KADAKAS_RUN_ID".to_string(), "1a".to_string())];
        let c = command_with(Path::new("/v/1.0.0"), &bundle, "", &files, &env).unwrap();
        assert_eq!(
            c.get_program(),
            Path::new("/v/1.0.0")
                .join("Kadakas Engine.app/Contents/MacOS/kadakas-app")
                .as_os_str()
        );
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, ["--", "/a/b.kad"]);
        assert_eq!(c.get_current_dir(), Some(Path::new("/v/1.0.0")));
    }

    #[cfg(unix)]
    #[test]
    fn a_program_starts_the_main_app_next_to_it_with_a_file_and_no_run() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("sibling");
        let app = d.join("kadakas-app");
        std::fs::write(
            &app,
            "#!/bin/sh\necho \"$2 $KADAKAS_VIA_LAUNCHER [$KADAKAS_RUN_ID]\" > out.txt\n",
        )
        .unwrap();
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
        let modeler = d.join("kadakas-modeler");
        let files = [OsString::from("/a/b.kad")];
        let mut c = sibling_app_command(&modeler, &files).expect("the app is next to it");
        assert_eq!(c.get_program(), app.as_os_str());
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, ["--", "/a/b.kad"]);
        // The run variables of the sender are taken out of the environment of the child, and the
        // marker that stops the handover to the launcher is put in.
        let envs: Vec<_> = c.get_envs().collect();
        assert!(
            envs.contains(&(OsStr::new("KADAKAS_RUN_ID"), None)),
            "{envs:?}"
        );
        assert!(
            envs.contains(&(OsStr::new("KADAKAS_LAUNCHER_ROOT"), None)),
            "{envs:?}"
        );
        assert!(envs.contains(&(OsStr::new("KADAKAS_VIA_LAUNCHER"), Some(OsStr::new("1")))));
        let mut child = c.spawn().unwrap();
        assert!(child.wait().unwrap().success());
        let out = std::fs::read_to_string(d.join("out.txt")).unwrap();
        assert!(out.starts_with("/a/b.kad 1 []"), "{out}");
        // Without the app next to the program there is no command.
        let lone = tmp("sibling-none");
        assert!(sibling_app_command(&lone.join("kadakas-modeler"), &files).is_none());
        std::fs::remove_dir_all(&d).unwrap();
        std::fs::remove_dir_all(&lone).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_goes_to_a_sibling_program_or_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("sibling-open");
        let exe = d.join("kadakas-app");
        let file = d.join("a.kad");
        // Not installed: refused, nothing starts.
        assert_eq!(
            open_in_sibling(None, "", &exe, "kadakas-modeler", &file),
            Err(SiblingError::Missing)
        );
        assert!(!sibling_exists(&exe, "kadakas-modeler"));
        let modeler = d.join("kadakas-modeler");
        std::fs::write(&modeler, "#!/bin/sh\necho \"$2\" > out.txt\n").unwrap();
        std::fs::set_permissions(&modeler, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(sibling_exists(&exe, "kadakas-modeler"));
        let c = sibling_command(&exe, "kadakas-modeler", &[file.as_os_str().to_owned()]).unwrap();
        let envs: Vec<_> = c.get_envs().collect();
        assert!(envs.contains(&(OsStr::new("KADAKAS_RUN_ID"), None)));
        // No program runs: it is started with the file.
        assert_eq!(
            open_in_sibling(None, "", &exe, "kadakas-modeler", &file),
            Ok(Sent::Spawned)
        );
        let out = d.join("out.txt");
        for _ in 0..200 {
            if out.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(&out).unwrap().trim(),
            file.to_string_lossy()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_sibling_that_runs_gets_the_file_as_an_open_request_and_no_second_process() {
        use crate::handshake::{AppLink, ENV_ROOT, ENV_RUN};
        let d = tmp("sibling-ask");
        // The program that runs is this test binary: it names itself beside its alive stamp.
        let me = std::env::current_exe()
            .unwrap()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        std::fs::write(d.join(&me), "x").unwrap();
        let root = d.to_string_lossy().into_owned();
        let env = move |k: &str| match k {
            ENV_ROOT => Some(root.clone()),
            ENV_RUN => Some("1a-1".to_string()),
            _ => None,
        };
        let link = AppLink::from_env(&env).unwrap();
        let _beat = link.start_heartbeat();
        let file = d.join("a.kad");
        let exe = d.join("kadakas-app");
        assert_eq!(
            open_in_sibling(Some(&d), "2b-2", &exe, &me, &file),
            Ok(Sent::Asked)
        );
        assert_eq!(link.take_open(), vec![file]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn no_profile_means_no_argument() {
        let c = command(Path::new("/v"), &flavor(), "").unwrap();
        assert_eq!(c.get_args().count(), 0);
        // The variable is removed, so a value from the launcher's own environment is not passed on.
        let env: Vec<_> = c.get_envs().collect();
        assert_eq!(env, [(OsStr::new("KADAKAS_PROFILE"), None)]);
    }

    #[test]
    fn bad_profile_is_refused() {
        assert!(command(Path::new("/v"), &flavor(), "a b; rm").is_err());
        assert!(launch(Path::new("/v"), &flavor(), "../x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn launch_runs_the_program() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("launch");
        let exe = d.join("kadakas-app");
        std::fs::write(
            &exe,
            "#!/bin/sh\necho \"$1 $2 $KADAKAS_PROFILE\" > out.txt\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = launch(&d, &flavor(), "vw").unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(
            std::fs::read_to_string(d.join("out.txt")).unwrap().trim(),
            "--profile vw vw"
        );
        // A missing program is an error.
        let gone = Flavor {
            exe: "nothing".into(),
            ..flavor()
        };
        assert!(launch(&d, &gone, "").is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
