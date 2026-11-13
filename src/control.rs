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
    #[error("No supported ThinkPad or Apple SMC fan interface was found on this computer.")]
    FanInterfaceMissing,
    #[error("The Apple SMC driver did not report a valid fan RPM range.")]
    InvalidFanRange,
    #[error("Could not set fan mode: {0}")]
    Write(#[source] io::Error),
}

#[derive(Clone, Debug)]
pub struct ControlPaths {
    pub proc_fan: PathBuf,
    pub fan_control: PathBuf,
    pub modprobe: PathBuf,
    pub apple_smc: PathBuf,
    pub dmi_product_name: PathBuf,
    pub dmi_product_version: PathBuf,
}

impl Default for ControlPaths {
    fn default() -> Self {
        Self {
            proc_fan: "/proc/acpi/ibm/fan".into(),
            fan_control: "/sys/module/thinkpad_acpi/parameters/fan_control".into(),
            modprobe: "/usr/sbin/modprobe".into(),
            apple_smc: "/sys/devices/platform/applesmc.768".into(),
            dmi_product_name: "/sys/class/dmi/id/product_name".into(),
            dmi_product_version: "/sys/class/dmi/id/product_version".into(),
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
        if arguments.first() == Some(&"thinkpad_acpi") && detail.contains("No such device") {
            return Err(ControlError::FanInterfaceMissing);
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    ThinkPad,
    AppleSmc,
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
        match self.detect_backend()? {
            Backend::ThinkPad => self.set_thinkpad_mode(mode),
            Backend::AppleSmc => self.set_apple_mode(mode),
        }
    }

    fn detect_backend(&self) -> Result<Backend, ControlError> {
        let apple = &self.paths.apple_smc;
        if ["fan1_manual", "fan1_output", "fan1_min", "fan1_max"]
            .iter()
            .all(|name| apple.join(name).exists())
        {
            return Ok(Backend::AppleSmc);
        }
        let is_thinkpad = [
            &self.paths.dmi_product_name,
            &self.paths.dmi_product_version,
        ]
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .any(|name| name.to_ascii_lowercase().contains("thinkpad"));
        if self.paths.proc_fan.exists() || self.paths.fan_control.exists() || is_thinkpad {
            return Ok(Backend::ThinkPad);
        }
        Err(ControlError::FanInterfaceMissing)
    }

    fn set_apple_mode(&self, mode: FanMode) -> Result<String, ControlError> {
        let apple = &self.paths.apple_smc;
        let manual = apple.join("fan1_manual");
        if mode == FanMode::Auto {
            if fs::read_to_string(&manual).ok().as_deref().map(str::trim) == Some("0") {
                return Ok("Automatic firmware control is already active.".to_owned());
            }
            fs::write(manual, "0\n").map_err(ControlError::Write)?;
            return Ok("Automatic fan control restored.".to_owned());
        }

        let read_rpm = |name| -> Result<u32, ControlError> {
            fs::read_to_string(apple.join(name))
                .map_err(ControlError::Write)?
                .trim()
                .parse()
                .map_err(|_| ControlError::InvalidFanRange)
        };
        let (min, max) = (read_rpm("fan1_min")?, read_rpm("fan1_max")?);
        let target = mode
            .apple_target_rpm(min, max)
            .ok_or(ControlError::InvalidFanRange)?;
        fs::write(apple.join("fan1_output"), format!("{target}\n")).map_err(ControlError::Write)?;
        fs::write(manual, "1\n").map_err(ControlError::Write)?;
        Ok(format!("Apple fan target set to {target} RPM."))
    }

    fn set_thinkpad_mode(&self, mode: FanMode) -> Result<String, ControlError> {
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
            apple_smc: root.join("applesmc"),
            dmi_product_name: root.join("product_name"),
            dmi_product_version: root.join("product_version"),
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

    #[test]
    fn apple_backend_sets_target_and_restores_auto_without_loading_thinkpad_module() {
        let (root, paths) = temporary_paths();
        fs::create_dir(&paths.apple_smc).unwrap();
        for (name, value) in [
            ("fan1_manual", "0\n"),
            ("fan1_output", "6000\n"),
            ("fan1_min", "1200\n"),
            ("fan1_max", "7200\n"),
        ] {
            fs::write(paths.apple_smc.join(name), value).unwrap();
        }
        let loader = FakeLoader::default();
        let controller = FanController::new(paths.clone(), loader.clone());
        assert_eq!(
            controller.set_mode(FanMode::Level4).unwrap(),
            "Apple fan target set to 4200 RPM."
        );
        assert_eq!(
            fs::read_to_string(paths.apple_smc.join("fan1_output")).unwrap(),
            "4200\n"
        );
        assert_eq!(
            fs::read_to_string(paths.apple_smc.join("fan1_manual")).unwrap(),
            "1\n"
        );
        assert_eq!(
            controller.set_mode(FanMode::Auto).unwrap(),
            "Automatic fan control restored."
        );
        assert_eq!(
            fs::read_to_string(paths.apple_smc.join("fan1_manual")).unwrap(),
            "0\n"
        );
        assert!(loader.0.lock().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_hardware_never_attempts_to_load_thinkpad_module() {
        let (root, paths) = temporary_paths();
        let loader = FakeLoader::default();
        let controller = FanController::new(paths, loader.clone());
        assert!(matches!(
            controller.set_mode(FanMode::Level7),
            Err(ControlError::FanInterfaceMissing)
        ));
        assert!(loader.0.lock().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recognizes_thinkpad_identity_before_module_is_loaded() {
        let (root, paths) = temporary_paths();
        fs::write(&paths.dmi_product_version, "ThinkPad X1 Carbon\n").unwrap();
        let controller = FanController::new(paths, FakeLoader::default());
        assert_eq!(
            controller.set_mode(FanMode::Auto).unwrap(),
            "Automatic firmware control is already active."
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_apple_range_does_not_enable_manual_mode() {
        let (root, paths) = temporary_paths();
        fs::create_dir(&paths.apple_smc).unwrap();
        for (name, value) in [
            ("fan1_manual", "0\n"),
            ("fan1_output", "6000\n"),
            ("fan1_min", "7200\n"),
            ("fan1_max", "1200\n"),
        ] {
            fs::write(paths.apple_smc.join(name), value).unwrap();
        }
        let controller = FanController::new(paths.clone(), FakeLoader::default());
        assert!(matches!(
            controller.set_mode(FanMode::Level1),
            Err(ControlError::InvalidFanRange)
        ));
        assert_eq!(
            fs::read_to_string(paths.apple_smc.join("fan1_manual")).unwrap(),
            "0\n"
        );
        assert_eq!(
            fs::read_to_string(paths.apple_smc.join("fan1_output")).unwrap(),
            "6000\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
