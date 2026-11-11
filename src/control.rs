use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use thiserror::Error;

use crate::mode::FanMode;

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("Invalid fan mode.")]
    InvalidMode,
    #[error("Could not run modprobe: {0}")]
    ModprobeIo(#[source] io::Error),
    #[error("{0}")]
    ModprobeFailed(String),
    #[error("thinkpad_acpi reloaded, but manual fan control is unavailable.")]
    ManualControlUnavailable,
    #[error("This device does not expose the ThinkPad fan interface.")]
    FanInterfaceMissing,
    #[error("Could not set fan mode: {0}")]
    Write(#[source] io::Error),
}

#[derive(Clone, Debug)]
pub struct ControlPaths {
    pub proc_fan: PathBuf,
    pub fan_control: PathBuf,
    pub modprobe: PathBuf,
}

impl Default for ControlPaths {
    fn default() -> Self {
        Self {
            proc_fan: "/proc/acpi/ibm/fan".into(),
            fan_control: "/sys/module/thinkpad_acpi/parameters/fan_control".into(),
            modprobe: "/usr/sbin/modprobe".into(),
        }
    }
}

pub trait ModuleLoader: Send + Sync {
    fn run(&self, executable: &Path, arguments: &[&str]) -> Result<(), ControlError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemModuleLoader;

impl ModuleLoader for SystemModuleLoader {
    fn run(&self, executable: &Path, arguments: &[&str]) -> Result<(), ControlError> {
        let output = Command::new(executable)
            .args(arguments)
            .output()
            .map_err(ControlError::ModprobeIo)?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(ControlError::ModprobeFailed(if detail.is_empty() {
            "Could not reload thinkpad_acpi.".to_owned()
        } else {
            detail
        }))
    }
}

/// Owns the complete kernel interaction for changing fan mode.
pub struct FanController<L = SystemModuleLoader> {
    paths: ControlPaths,
    loader: L,
}

impl Default for FanController<SystemModuleLoader> {
    fn default() -> Self {
        Self::new(ControlPaths::default(), SystemModuleLoader)
    }
}

impl<L: ModuleLoader> FanController<L> {
    pub fn new(paths: ControlPaths, loader: L) -> Self {
        Self { paths, loader }
    }

    pub fn set_mode(&self, mode: FanMode) -> Result<String, ControlError> {
        if mode == FanMode::Auto && !self.fan_control_enabled() {
            return Ok("Automatic firmware control is already active.".to_owned());
        }

        if mode.is_manual() {
            self.enable_manual_control()?;
        }
        if !self.paths.proc_fan.exists() {
            return Err(ControlError::FanInterfaceMissing);
        }

        fs::write(
            &self.paths.proc_fan,
            format!("level {}\n", mode.kernel_value()),
        )
        .map_err(ControlError::Write)?;

        Ok(match mode {
            FanMode::Auto => "Automatic fan control restored.".to_owned(),
            FanMode::FullSpeed => "Full-speed cooling enabled.".to_owned(),
            _ => format!("Manual fan level {} enabled.", mode.kernel_value()),
        })
    }

    fn fan_control_enabled(&self) -> bool {
        matches!(
            fs::read_to_string(&self.paths.fan_control)
                .ok()
                .as_deref()
                .map(str::trim),
            Some("Y" | "1")
        )
    }

    fn enable_manual_control(&self) -> Result<(), ControlError> {
        if self.fan_control_enabled() {
            return Ok(());
        }
        self.loader
            .run(&self.paths.modprobe, &["-r", "thinkpad_acpi"])?;
        if let Err(error) = self
            .loader
            .run(&self.paths.modprobe, &["thinkpad_acpi", "fan_control=1"])
        {
            let _ = self.loader.run(&self.paths.modprobe, &["thinkpad_acpi"]);
            return Err(error);
        }
        for _ in 0..20 {
            if self.paths.proc_fan.exists() && self.fan_control_enabled() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(ControlError::ManualControlUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[derive(Clone, Default)]
    struct FakeLoader(Arc<Mutex<Vec<Vec<String>>>>);

    impl ModuleLoader for FakeLoader {
        fn run(&self, _executable: &Path, arguments: &[&str]) -> Result<(), ControlError> {
            self.0
                .lock()
                .unwrap()
                .push(arguments.iter().map(|value| (*value).to_owned()).collect());
            Ok(())
        }
    }

    fn temporary_paths() -> (PathBuf, ControlPaths) {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fanspeed-control-{suffix}"));
        fs::create_dir_all(&root).unwrap();
        let paths = ControlPaths {
            proc_fan: root.join("fan"),
            fan_control: root.join("fan_control"),
            modprobe: "/fake/modprobe".into(),
        };
        (root, paths)
    }

    #[test]
    fn auto_is_a_noop_when_manual_control_is_disabled() {
        let (root, paths) = temporary_paths();
        fs::write(&paths.fan_control, "N\n").unwrap();
        let controller = FanController::new(paths, FakeLoader::default());
        assert_eq!(
            controller.set_mode(FanMode::Auto).unwrap(),
            "Automatic firmware control is already active."
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_only_a_validated_kernel_command() {
        let (root, paths) = temporary_paths();
        fs::write(&paths.fan_control, "Y\n").unwrap();
        fs::write(&paths.proc_fan, "").unwrap();
        let controller = FanController::new(paths.clone(), FakeLoader::default());
        assert_eq!(
            controller.set_mode(FanMode::Level7).unwrap(),
            "Manual fan level 7 enabled."
        );
        assert_eq!(fs::read_to_string(paths.proc_fan).unwrap(), "level 7\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auto_restores_firmware_control() {
        let (root, paths) = temporary_paths();
        fs::write(&paths.fan_control, "1\n").unwrap();
        fs::write(&paths.proc_fan, "").unwrap();
        let controller = FanController::new(paths.clone(), FakeLoader::default());
        assert_eq!(
            controller.set_mode(FanMode::Auto).unwrap(),
            "Automatic fan control restored."
        );
        assert_eq!(fs::read_to_string(paths.proc_fan).unwrap(), "level auto\n");
        fs::remove_dir_all(root).unwrap();
    }
}
