//! Starting the app. The profile goes to the app twice: as `--profile NAME` and as the variable
//! `KADAKAS_PROFILE`. The app reads either.

use crate::error::{Result, UpdateError};
use crate::flavors::Flavor;
use crate::settings::valid_name;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// The command that starts `flavor` from the version folder `dir`.
/// An empty profile means no profile. A bad profile name is an error.
pub fn command(dir: &Path, flavor: &Flavor, profile: &str) -> Result<Command> {
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
    Ok(cmd)
}

/// Starts the app and does not wait for it.
pub fn launch(dir: &Path, flavor: &Flavor, profile: &str) -> Result<Child> {
    Ok(command(dir, flavor, profile)?.spawn()?)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            Path::new("/v/1.0.0/kadakas-app").as_os_str()
        );
        let env: Vec<_> = c.get_envs().collect();
        assert_eq!(
            env,
            [(OsStr::new("KADAKAS_PROFILE"), Some(OsStr::new("acad")))]
        );
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
